//! Idle-loop housekeeping: hot reload, pty polling, debounced saves, and wakeup scheduling.

use super::events::{
    AGENT_SCAN_INTERVAL, CONFIG_SAVE_DEBOUNCE, LIVENESS_POLL_INTERVAL, SCROLLBAR_FADE_AFTER,
};
#[cfg(target_os = "macos")]
use super::macos::{
    center_macos_window_title, get_nswindow, set_macos_tab_label, style_macos_tab_bar,
};
use super::App;
use std::time::Instant;
use tracing::{error, info};
use winit::event_loop::{ActiveEventLoop, ControlFlow};

impl App {
    /// Body of [`winit::application::ApplicationHandler::about_to_wait`].
    pub(crate) fn on_about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "macos")]
        self.poll_macos_editor();

        // The titlebar "AI" button is an AppKit control, so its clicks arrive
        // outside winit's event stream. Pick them up here.
        #[cfg(target_os = "macos")]
        {
            let clicks = crate::macos_titlebar::click_count();
            if clicks != self.last_agent_toggle_click {
                self.last_agent_toggle_click = clicks;
                self.toggle_sidebar();
            }
        }

        // 1. Hot Reload Configuration
        if let Some(reloaded_cfg) = self.config_manager.check_reload().cloned() {
            info!("Config file modified on disk, applying live changes to all sessions...");
            // Came from disk -- do NOT write it back, or we retrigger the watcher.
            self.apply_config_live(&reloaded_cfg, false);
        }

        // 1.5 Scan AI agents periodically (every 1.5s) to avoid scanning during
        // RedrawRequested. Only meaningful while the sidebar is visible -- with it
        // closed nothing reads `cached_ai_agents`, and the scan walks the whole
        // process tree, so doing it anyway would keep the machine awake for nothing.
        if self.sidebar_open {
            let now_scan = Instant::now();
            let active_output_expired = self.sessions.values().any(|s| {
                s.workspace
                    .next_active_output_expiry(now_scan, self.last_ai_agent_scan)
                    .is_some_and(|at| at <= now_scan)
            });
            let need_scan = active_output_expired
                || match self.last_ai_agent_scan {
                    Some(last) => now_scan.duration_since(last) >= AGENT_SCAN_INTERVAL,
                    None => true,
                };
            if need_scan {
                self.refresh_agent_cache();
            }
        }

        // 2. Poll all sessions for PTY output and dead sessions
        let tab_bar_h = self.get_tab_bar_height();
        let mut dead_sessions = Vec::new();
        let now = Instant::now();
        // Earliest moment at which we have timed work to do. `None` means "sleep
        // until an actual event arrives".
        let mut next_deadline: Option<Instant> = None;
        let mut arm = |at: Instant| {
            next_deadline = Some(match next_deadline {
                Some(cur) if cur <= at => cur,
                _ => at,
            });
        };

        // Read before the loop: the loop takes `&mut self.sessions`, and these
        // live on other fields.
        let active_session = self.active_session_id;
        #[cfg(target_os = "macos")]
        let active_win_ptr = active_session
            .and_then(|aid| self.sessions.get(&aid))
            .map(|s| get_nswindow(&s.window))
            .unwrap_or(std::ptr::null_mut());
        let configured_shell: Option<&str> = self
            .config_manager
            .current_config
            .terminal
            .shell
            .as_ref()
            .map(|s| s.program.as_str());

        let mut any_program_status_changed = false;

        for (id, session) in self.sessions.iter_mut() {
            if session.workspace.has_dead_sessions() {
                let all_dead = session.workspace.cleanup_dead_sessions();
                if all_dead {
                    dead_sessions.push(*id);
                    continue;
                } else {
                    session.sync_all_pane_sizes(tab_bar_h);
                    session.request_redraw();
                }
            } else {
                let outcome = session.workspace.poll_all_ptys();
                if outcome.any_output {
                    session.flush_osc52_clipboard(self.clipboard.as_ref());
                }
                if outcome.any_output && active_session != Some(*id) {
                    session.mark_unseen_output();
                }
                if outcome.program_status_changed {
                    session.last_tab_label_refresh = None;
                    any_program_status_changed = true;
                }
                if outcome.needs_redraw() {
                    session.request_output_redraw();
                }
            }

            // Whatever held a frame back -- this poll or the `PtyWakeup` arm --
            // the deferred frame is owed a deadline, or output stopping mid-burst
            // would leave the last chunk undrawn.
            session.refresh_frame_interval();
            if let Some(due) = session.flush_paced_redraw() {
                arm(due);
            }

            if let Some((label, win_title, tooltip)) = session.refresh_tab_label(configured_shell) {
                if session.window.title() != win_title {
                    session.window.set_title(&win_title);
                }
                #[cfg(target_os = "macos")]
                {
                    let win_ptr = get_nswindow(&session.window);
                    unsafe {
                        set_macos_tab_label(win_ptr, &label, &tooltip);
                        center_macos_window_title(win_ptr);
                        let target_active = if !active_win_ptr.is_null() {
                            active_win_ptr
                        } else {
                            win_ptr
                        };
                        style_macos_tab_bar(target_active);
                    }
                }
                #[cfg(not(target_os = "macos"))]
                let _ = (label, tooltip);
            }

            // The scrollbar is shown for SCROLLBAR_FADE_AFTER after the last scroll and
            // then simply disappears -- there is no interpolation, so a single redraw at
            // the boundary is all that is needed.
            if let Some(last) = session.last_scroll_time {
                let deadline = last + SCROLLBAR_FADE_AFTER;
                if now >= deadline {
                    session.last_scroll_time = None;
                    session.request_redraw();
                } else {
                    arm(deadline);
                }
            }
        }

        for id in dead_sessions {
            self.record_session_window_position(id);
            self.sessions.remove(&id);
            self.session_order.retain(|&wid| wid != id);
        }

        if self.sessions.is_empty() {
            info!("All terminal sessions closed. Exiting skTerm.");
            event_loop.exit();
            return;
        }

        if any_program_status_changed && self.sidebar_open {
            self.refresh_agent_cache();
        }

        // On macOS, `winit` drains `pending_redraw` *before* calling `about_to_wait`,
        // and `CFRunLoopWakeUp` called inside `about_to_wait` (`kCFRunLoopBeforeWaiting`)
        // is a no-op because CoreFoundation has not yet set `__CFRunLoopSetSleeping`.
        // If any session queued a redraw during `about_to_wait` (or still has dirty
        // visible terminal cells ready to draw), arm `now` so `EventLoopWaker` sets
        // its `CFRunLoopTimer` fire date to `f64::MIN` and the runloop turns around
        // immediately to service `WindowEvent::RedrawRequested`.
        for session in self.sessions.values_mut() {
            session.sync_focus_events();
            if session.needs_immediate_redraw() {
                if !session.redraw_queued {
                    session.request_redraw();
                }
                arm(now);
            }
        }

        if let Some(pref) = &mut self.preferences_window {
            if let Some(due) = pref.next_repaint_at {
                if now >= due {
                    pref.next_repaint_at = None;
                    pref.is_occluded = false;
                    pref.needs_redraw = true;
                } else {
                    arm(due);
                }
            }
            if pref.needs_redraw && !pref.is_occluded {
                pref.window.request_redraw();
                arm(now);
            }
        }

        // 3. Debounced config auto-save. This is the only place that writes
        // config.toml while running, which is what keeps the file consistent.
        if self.pending_config_save {
            if let Some(last) = self.last_config_change {
                let deadline = last + CONFIG_SAVE_DEBOUNCE;
                if now >= deadline {
                    self.pending_config_save = false;
                    match toml::to_string_pretty(&self.config_manager.current_config) {
                        Ok(toml_str) => {
                            let path = self.config_manager.path.clone();
                            std::thread::spawn(move || {
                                if let Err(e) =
                                    crate::config::write_config_atomically(&path, &toml_str)
                                {
                                    error!("Failed to save config to {:?}: {}", path, e);
                                }
                            });
                        }
                        Err(e) => error!("Failed to serialize config: {}", e),
                    }
                } else {
                    arm(deadline);
                }
            }
        }

        if self.sidebar_open {
            arm(self
                .last_ai_agent_scan
                .map_or(now, |last| last + AGENT_SCAN_INTERVAL));
            for session in self.sessions.values() {
                if let Some(expiry) = session
                    .workspace
                    .next_active_output_expiry(now, self.last_ai_agent_scan)
                {
                    arm(expiry);
                }
            }
        }

        // A child can exit without the pty ever reaching EOF (e.g. another process is
        // holding the slave fd open), in which case no wakeup arrives and the pane would
        // linger forever. A slow backstop poll costs one throttled `waitpid` per pane per
        // second -- negligible next to the 60 Hz tick this replaces.
        arm(now + LIVENESS_POLL_INTERVAL);

        event_loop.set_control_flow(match next_deadline {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    /// Body of [`winit::application::ApplicationHandler::exiting`].
    pub(crate) fn on_exiting(&mut self) {
        if let Some(active_id) = self.active_session_id {
            self.record_session_window_position(active_id);
        }
        // Saves are debounced, so a change made in the last few hundred
        // milliseconds before quitting would otherwise never reach disk. Write it
        // synchronously here rather than on a thread we are about to outlive.
        if self.pending_config_save {
            self.pending_config_save = false;
            match toml::to_string_pretty(&self.config_manager.current_config) {
                Ok(toml_str) => {
                    if let Err(e) =
                        crate::config::write_config_atomically(&self.config_manager.path, &toml_str)
                    {
                        error!("Failed to flush config on exit: {}", e);
                    }
                }
                Err(e) => error!("Failed to serialize config on exit: {}", e),
            }
        }
    }
}
