//! Custom application events and event-loop timing constants.

use std::time::Duration;

/// Custom application events for asynchronous wakeups.
#[derive(Debug, Clone, Copy)]
pub enum AppEvent {
    PtyWakeup,
    /// Sent from the config file-watcher thread. The loop now sleeps in
    /// `ControlFlow::Wait`, so the watcher has to wake it explicitly or a hot
    /// reload would sit in the channel until the next unrelated event.
    ConfigChanged,
}

/// How long the scrollbar stays visible after the last scroll.
pub(crate) const SCROLLBAR_FADE_AFTER: Duration = Duration::from_millis(1500);
/// Cadence of the AI-agent process-tree rescan (only while the sidebar is open).
pub(crate) const AGENT_SCAN_INTERVAL: Duration = Duration::from_millis(1500);
/// Debounce before persisting config.toml after a geometry or preference change.
pub(crate) const CONFIG_SAVE_DEBOUNCE: Duration = Duration::from_millis(400);
/// Backstop cadence for reaping panes whose child exited without a pty EOF.
pub(crate) const LIVENESS_POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// Cadence of native-tab label recomputation.
///
/// Recomputing means two process lookups per session, and a tab label that lags
/// a `cd` by half a second is not something a user can perceive.
pub(crate) const TAB_LABEL_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
/// Longest gap between two clicks that still counts as a double click.
///
/// Shared by the terminal's click-count state machine and the agent panel's
/// rename gesture, but they track their *timestamps* separately -- see
/// [`App::last_agent_card_click`](super::App::last_agent_card_click).
pub(crate) const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);
/// Minimum gap between pane reflows while a splitter is actively being dragged.
///
/// `Terminal::resize` already no-ops when the cell dimensions are unchanged, but a
/// drag across the window still crosses one column boundary every `cell_w` pixels,
/// and each crossing reflows the whole scrollback *and* fires a `SIGWINCH` at the
/// child. Full-screen apps (vim, htop) repaint on every one of those, so a fast drag
/// turns into dozens of full redraws in the child process.
///
/// The splitter itself still tracks the mouse at full rate -- only the terminal
/// re-layout is throttled -- and `MouseInput`'s release arm always performs a final
/// unthrottled sync, so the end state is exact.
pub(crate) const RESIZE_REFLOW_DEBOUNCE: Duration = Duration::from_millis(50);
/// Frame interval assumed when the monitor does not report a refresh rate.
///
/// Erring on the *fast* side is the safe direction: a too-short interval only
/// weakens the throttle, while a too-long one would drop frames the display
/// could actually have shown. Hence nanoseconds rather than `from_micros(16_667)`
/// -- rounding the period *up* costs a whole frame every second.
pub(crate) const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_666);
/// How often a session re-reads the refresh rate of the monitor it sits on.
///
/// The rate only changes when the window is dragged to another display (or the
/// user edits the display settings), so polling it is far cheaper than wiring up
/// `Moved`, which fires continuously during a window drag.
pub(crate) const FRAME_INTERVAL_REFRESH: Duration = Duration::from_secs(1);
