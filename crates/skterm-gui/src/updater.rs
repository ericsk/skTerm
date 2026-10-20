//! Cross-platform Over-The-Air (OTA) self-updater for `skTerm`.
//!
//! Queries GitHub Releases (`https://api.github.com/repos/ericsk/skTerm/releases/latest`),
//! compares semantic versions against `CARGO_PKG_VERSION`, verifies downloaded archives
//! using a built-in FIPS 180-4 SHA-256 implementation against `SHA256SUMS.txt`, and
//! performs atomic in-place replacement of the macOS `.app` bundle or standalone binary.

use crate::app::AppEvent;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use tracing::{error, info, warn};
use winit::event_loop::EventLoopProxy;

/// Current compiled version of `skTerm`.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// GitHub repository slug (`owner/repo`).
pub const GITHUB_REPO: &str = "ericsk/skTerm";

/// GitHub API endpoint for the latest published release.
pub const GITHUB_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/ericsk/skTerm/releases/latest";

/// Public web URL for browsing releases manually.
pub const GITHUB_RELEASES_PAGE: &str = "https://github.com/ericsk/skTerm/releases";

/// Parsed semantic version (`major.minor.patch` with optional pre-release tag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl SemVer {
    /// Parse a version string such as `"v0.1.0"`, `"0.2.1"`, or `"1.0.0-beta.1"`.
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim().strip_prefix(['v', 'V']).unwrap_or(raw.trim());
        // Strip build metadata (+...)
        let without_build = trimmed.split('+').next().unwrap_or(trimmed);
        let (core, pre) = match without_build.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some((c, _)) => (c, None),
            None => (without_build, None),
        };

        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }

        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)) {
            std::cmp::Ordering::Equal => match (&self.pre, &other.pre) {
                (None, None) => std::cmp::Ordering::Equal,
                // A normal release (no pre-release tag) is newer than a pre-release of the same version
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            },
            ord => ord,
        }
    }
}

/// Returns `true` if `remote_tag` represents a strictly newer version than `current_ver`.
pub fn is_newer_version(current_ver: &str, remote_tag: &str) -> bool {
    match (SemVer::parse(current_ver), SemVer::parse(remote_tag)) {
        (Some(cur), Some(rem)) => rem > cur,
        _ => false,
    }
}

/// Metadata for a single downloadable asset attached to a GitHub Release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

/// Metadata for a GitHub Release relevant to the current platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
    pub html_url: String,
    pub body: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub checksum_url: Option<String>,
}

/// Lifecycle state of the background OTA updater.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate {
        version: String,
    },
    Available(ReleaseInfo),
    Downloading {
        info: ReleaseInfo,
        stage: String,
    },
    ReadyToRestart {
        version: String,
        target_path: PathBuf,
    },
    Failed(String),
}

static UPDATE_STATUS: OnceLock<Mutex<UpdateStatus>> = OnceLock::new();
static UPDATE_EVENT_PROXY: OnceLock<Mutex<Option<EventLoopProxy<AppEvent>>>> = OnceLock::new();

/// Register the [`EventLoopProxy`] so background update threads can wake the GUI immediately.
pub fn set_update_event_proxy(proxy: EventLoopProxy<AppEvent>) {
    let lock = UPDATE_EVENT_PROXY.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = Some(proxy);
    }
}

/// Read a snapshot of the current [`UpdateStatus`].
pub fn current_update_status() -> UpdateStatus {
    let lock = UPDATE_STATUS.get_or_init(|| Mutex::new(UpdateStatus::Idle));
    match lock.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Update the global [`UpdateStatus`] and notify the event loop.
pub fn set_update_status(new_status: UpdateStatus) {
    let lock = UPDATE_STATUS.get_or_init(|| Mutex::new(UpdateStatus::Idle));
    if let Ok(mut guard) = lock.lock() {
        *guard = new_status;
    }
    if let Some(proxy_lock) = UPDATE_EVENT_PROXY.get() {
        if let Ok(guard) = proxy_lock.lock() {
            if let Some(proxy) = guard.as_ref() {
                let _ = proxy.send_event(AppEvent::UpdateStateChanged);
            }
        }
    }
}

// ============================================================================
// Pure-Rust FIPS 180-4 SHA-256 Implementation
// ============================================================================

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buf_len: usize,
    total_bits: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buf_len: 0,
            total_bits: 0,
        }
    }

    fn compress_block(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*chunk);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_bits = self.total_bits.wrapping_add((data.len() as u64) * 8);

        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            if data.len() < need {
                self.buffer[self.buf_len..self.buf_len + data.len()].copy_from_slice(data);
                self.buf_len += data.len();
                return;
            }
            self.buffer[self.buf_len..64].copy_from_slice(&data[..need]);
            Self::compress_block(&mut self.state, &self.buffer);
            self.buf_len = 0;
            data = &data[need..];
        }

        while data.len() >= 64 {
            let block: &[u8; 64] = data[..64].try_into().unwrap();
            Self::compress_block(&mut self.state, block);
            data = &data[64..];
        }

        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn finalize_hex(mut self) -> String {
        self.buffer[self.buf_len] = 0x80;
        self.buf_len += 1;

        if self.buf_len > 56 {
            for b in &mut self.buffer[self.buf_len..64] {
                *b = 0;
            }
            Self::compress_block(&mut self.state, &self.buffer);
            self.buf_len = 0;
        }

        for b in &mut self.buffer[self.buf_len..56] {
            *b = 0;
        }
        self.buffer[56..64].copy_from_slice(&self.total_bits.to_be_bytes());
        Self::compress_block(&mut self.state, &self.buffer);

        let mut out = String::with_capacity(64);
        for word in self.state {
            out.push_str(&format!("{:08x}", word));
        }
        out
    }
}

/// Compute lowercase hex-encoded SHA-256 digest of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize_hex()
}

/// Compute lowercase hex-encoded SHA-256 digest of a file on disk.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize_hex())
}

// ============================================================================
// GitHub Release JSON & Checksum Parsing
// ============================================================================

/// Extract the value of a JSON string field `"key": "value"` from `src`.
fn extract_json_string_field(src: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let mut search_from = 0;
    while let Some(rel_idx) = src[search_from..].find(&pattern) {
        let after_key = search_from + rel_idx + pattern.len();
        let rest = src[after_key..].trim_start();
        let Some(after_colon) = rest.strip_prefix(':') else {
            search_from = after_key;
            continue;
        };
        let after_colon = after_colon.trim_start();
        if let Some(after_quote) = after_colon.strip_prefix('"') {
            return parse_json_escaped_string(after_quote);
        }
        search_from = after_key;
    }
    None
}

fn parse_json_escaped_string(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => {
                let esc = chars.next()?;
                match esc {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let mut hex = String::with_capacity(4);
                        for _ in 0..4 {
                            hex.push(chars.next()?);
                        }
                        if let Ok(code) = u16::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(code as u32) {
                                out.push(ch);
                            }
                        }
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
    }
    None
}

/// Extract all `{ ... }` objects inside `"assets": [ ... ]` from a GitHub Release JSON payload.
fn extract_json_assets(src: &str) -> Vec<ReleaseAsset> {
    let mut assets = Vec::new();
    let Some(assets_key_idx) = src.find("\"assets\"") else {
        return assets;
    };
    let after_key = &src[assets_key_idx + "\"assets\"".len()..];
    let Some(bracket_start) = after_key.find('[') else {
        return assets;
    };
    let array_slice = &after_key[bracket_start + 1..];

    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut obj_start: Option<usize> = None;

    for (idx, ch) in array_slice.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    obj_start = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = obj_start.take() {
                        let obj_str = &array_slice[start..=idx];
                        if let (Some(name), Some(url)) = (
                            extract_json_string_field(obj_str, "name"),
                            extract_json_string_field(obj_str, "browser_download_url"),
                        ) {
                            assets.push(ReleaseAsset {
                                name,
                                browser_download_url: url,
                            });
                        }
                    }
                }
            }
            ']' if depth == 0 => break,
            _ => {}
        }
    }

    assets
}

/// Choose the most suitable OTA archive for `(os, arch)` from the release's assets.
pub fn select_platform_asset<'a>(
    assets: &'a [ReleaseAsset],
    os: &str,
    arch: &str,
) -> Option<&'a ReleaseAsset> {
    let arch_tokens: &[&str] = match arch {
        "aarch64" | "arm64" => &["arm64", "aarch64", "universal"],
        "x86_64" | "amd64" => &["x86_64", "x64", "amd64", "universal"],
        other => &[other],
    };

    let matches_arch =
        |name_lower: &str| -> bool { arch_tokens.iter().any(|t| name_lower.contains(t)) };

    match os {
        "macos" => {
            // 1. Prefer .app.tar.gz or .tar.gz for matching architecture
            if let Some(a) = assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                (n.contains("macos") || n.contains("darwin") || n.contains("apple"))
                    && n.ends_with(".tar.gz")
                    && matches_arch(&n)
            }) {
                return Some(a);
            }
            // 2. Fallback to .dmg for matching architecture
            if let Some(a) = assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                n.ends_with(".dmg") && matches_arch(&n)
            }) {
                return Some(a);
            }
            // 3. Any macOS .tar.gz or .dmg
            assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                n.ends_with(".dmg")
                    || ((n.contains("macos") || n.contains("darwin")) && n.ends_with(".tar.gz"))
            })
        }
        "windows" => {
            // 1. Prefer portable .zip for matching architecture
            if let Some(a) = assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                n.contains("windows") && n.ends_with(".zip") && matches_arch(&n)
            }) {
                return Some(a);
            }
            // 2. Any Windows .zip or standalone .exe (prefer non-setup if possible)
            assets
                .iter()
                .find(|a| {
                    let n = a.name.to_lowercase();
                    n.contains("windows") && n.ends_with(".zip")
                })
                .or_else(|| {
                    assets.iter().find(|a| {
                        let n = a.name.to_lowercase();
                        n.contains("windows") && n.ends_with(".exe") && !n.contains("setup")
                    })
                })
        }
        "linux" => {
            // 1. Prefer portable .tar.gz for matching architecture
            if let Some(a) = assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                n.contains("linux") && n.ends_with(".tar.gz") && matches_arch(&n)
            }) {
                return Some(a);
            }
            // 2. Fallback to .deb
            assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                (n.contains("linux") && n.ends_with(".tar.gz")) || n.ends_with(".deb")
            })
        }
        _ => None,
    }
}

/// Parse GitHub's `/releases/latest` JSON response into [`ReleaseInfo`] for `(os, arch)`.
pub fn parse_github_release_json_for_platform(
    json: &str,
    os: &str,
    arch: &str,
) -> Result<ReleaseInfo, String> {
    let tag_name = extract_json_string_field(json, "tag_name")
        .ok_or_else(|| "Missing 'tag_name' in GitHub release JSON".to_string())?;
    let version = tag_name
        .trim()
        .strip_prefix(['v', 'V'])
        .unwrap_or(tag_name.trim())
        .to_string();
    let html_url = extract_json_string_field(json, "html_url")
        .unwrap_or_else(|| GITHUB_RELEASES_PAGE.to_string());
    let body = extract_json_string_field(json, "body").unwrap_or_default();

    let assets = extract_json_assets(json);
    let chosen = select_platform_asset(&assets, os, arch);

    let checksum_url = chosen.and_then(|target| {
        let exact_sha_name = format!("{}.sha256", target.name);
        assets
            .iter()
            .find(|a| {
                let n = a.name.to_lowercase();
                a.name.eq_ignore_ascii_case(&exact_sha_name)
                    || n == "sha256sums.txt"
                    || n == "sha256sums"
                    || n == "checksums.txt"
            })
            .map(|a| a.browser_download_url.clone())
    });

    Ok(ReleaseInfo {
        tag_name,
        version,
        html_url,
        body,
        asset_name: chosen.map(|a| a.name.clone()),
        asset_url: chosen.map(|a| a.browser_download_url.clone()),
        checksum_url,
    })
}

/// Parse a `SHA256SUMS.txt` or `.sha256` file and return the 64-character hex digest for `target_filename`.
pub fn parse_sha256sums(content: &str, target_filename: &str) -> Option<String> {
    let target_trimmed = target_filename.trim();
    let mut single_hash_fallback: Option<String> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(first) = parts.next() else {
            continue;
        };
        if first.len() == 64 && first.chars().all(|c| c.is_ascii_hexdigit()) {
            let hash = first.to_lowercase();
            if let Some(raw_file) = parts.next() {
                let file_clean = raw_file.trim_start_matches('*');
                let basename = Path::new(file_clean)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(file_clean);
                if file_clean == target_trimmed || basename == target_trimmed {
                    return Some(hash);
                }
            } else if single_hash_fallback.is_none() {
                single_hash_fallback = Some(hash);
            }
        }
    }

    single_hash_fallback
}

// ============================================================================
// Bundle & Binary Path Resolution + Atomic In-Place Replacement
// ============================================================================

/// Given an executable path (e.g. `/Applications/skTerm.app/Contents/MacOS/skterm`),
/// returns the enclosing `.app` bundle directory (`/Applications/skTerm.app`) if applicable.
pub fn resolve_macos_app_bundle_from_exe(exe_path: &Path) -> Option<PathBuf> {
    let macos_dir = exe_path.parent()?;
    if macos_dir.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app_dir = contents_dir.parent()?;
    if app_dir.extension()?.to_str()? == "app" {
        Some(app_dir.to_path_buf())
    } else {
        None
    }
}

/// Remove any leftover `<exe>.old` or `<app>.old` created during a previous OTA update.
pub fn cleanup_old_executable_on_startup() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let old_exe = exe.with_extension("old");
    if old_exe.exists() {
        let _ = fs::remove_file(&old_exe);
    }
    if let Some(app_bundle) = resolve_macos_app_bundle_from_exe(&exe) {
        let old_bundle = app_bundle.with_extension("app.old");
        if old_bundle.exists() {
            let _ = fs::remove_dir_all(&old_bundle);
        }
    }
}

/// Atomically replace a standalone executable file (`target_exe`) with `new_binary`.
///
/// Renames `target_exe` to `<target_exe>.old` before moving `new_binary` into place,
/// which works on both Unix and Windows even while `target_exe` is currently running.
pub fn replace_executable_in_place(target_exe: &Path, new_binary: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(new_binary) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(new_binary, perms);
        }
    }

    let backup_exe = target_exe.with_extension("old");
    if backup_exe.exists() {
        let _ = fs::remove_file(&backup_exe);
    }

    if target_exe.exists() {
        fs::rename(target_exe, &backup_exe).map_err(|e| {
            format!(
                "無法重新命名現有執行檔 {:?} -> {:?}: {}",
                target_exe, backup_exe, e
            )
        })?;
    }

    if let Err(e) = fs::copy(new_binary, target_exe) {
        // Rollback if copy failed
        if backup_exe.exists() {
            let _ = fs::rename(&backup_exe, target_exe);
        }
        return Err(format!("無法寫入新版執行檔至 {:?}: {}", target_exe, e));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(target_exe) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(target_exe, perms);
        }
        let _ = fs::remove_file(&backup_exe);
    }

    Ok(())
}

/// Atomically replace a macOS `.app` bundle (`target_app`) with `staged_new_app`.
pub fn replace_macos_app_bundle_in_place(
    target_app: &Path,
    staged_new_app: &Path,
) -> Result<(), String> {
    if !staged_new_app.join("Contents").join("MacOS").exists() {
        return Err(format!(
            "更新套件結構無效，缺少 Contents/MacOS: {:?}",
            staged_new_app
        ));
    }

    // Clear quarantine/provenance xattrs and ad-hoc sign if needed so Gatekeeper is happy
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("xattr")
            .arg("-cr")
            .arg(staged_new_app)
            .status();
        let verify = Command::new("codesign")
            .arg("--verify")
            .arg("--deep")
            .arg("--strict")
            .arg(staged_new_app)
            .status();
        if !verify.is_ok_and(|s| s.success()) {
            let _ = Command::new("codesign")
                .args(["--force", "--deep", "--sign", "-"])
                .arg(staged_new_app)
                .status();
        }
    }

    let parent = target_app
        .parent()
        .ok_or_else(|| "無法取得目標 .app 所在目錄".to_string())?;
    let local_staging = parent.join(".skTerm.app.updating");
    let backup_app = target_app.with_extension("app.old");

    if local_staging.exists() {
        let _ = fs::remove_dir_all(&local_staging);
    }
    if backup_app.exists() {
        let _ = fs::remove_dir_all(&backup_app);
    }

    // Copy into the same filesystem directory as target_app so `rename` is atomic
    let cp_status = Command::new("cp")
        .arg("-R")
        .arg(staged_new_app)
        .arg(&local_staging)
        .status()
        .map_err(|e| format!("無法複製更新套件至目標目錄: {}", e))?;
    if !cp_status.success() {
        let _ = fs::remove_dir_all(&local_staging);
        return Err("無法寫入應用程式目錄（權限不足）".to_string());
    }

    if target_app.exists() {
        if let Err(e) = fs::rename(target_app, &backup_app) {
            let _ = fs::remove_dir_all(&local_staging);
            return Err(format!("無法替換現有應用程式 {:?}: {}", target_app, e));
        }
    }

    if let Err(e) = fs::rename(&local_staging, target_app) {
        if backup_app.exists() {
            let _ = fs::rename(&backup_app, target_app);
        }
        return Err(format!("無法啟用新版應用程式 {:?}: {}", target_app, e));
    }

    let _ = fs::remove_dir_all(&backup_app);
    Ok(())
}

// ============================================================================
// Network & Archive Execution Helpers
// ============================================================================

fn fetch_url_text(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "15",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            &format!("User-Agent: skTerm/{}", CURRENT_VERSION),
            url,
        ])
        .output()
        .map_err(|e| format!("無法執行 curl 檢查更新: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "連線至更新伺服器失敗 ({}): {}",
            output.status,
            stderr.trim()
        ));
    }

    String::from_utf8(output.stdout).map_err(|e| format!("回應內容非有效 UTF-8: {}", e))
}

fn download_url_to_file(url: &str, dest: &Path) -> Result<(), String> {
    let status = Command::new("curl")
        .args([
            "-fL",
            "--max-time",
            "180",
            "-H",
            &format!("User-Agent: skTerm/{}", CURRENT_VERSION),
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| format!("無法執行 curl 下載更新檔: {}", e))?;

    if !status.success() {
        return Err(format!("下載更新檔失敗 (HTTP status: {})", status));
    }
    Ok(())
}

/// Spawn a non-blocking background thread to check GitHub Releases for a newer version.
pub fn trigger_check_for_updates(force_check: bool) {
    if !force_check
        && matches!(
            current_update_status(),
            UpdateStatus::Checking | UpdateStatus::Downloading { .. }
        )
    {
        return;
    }

    set_update_status(UpdateStatus::Checking);

    std::thread::spawn(move || {
        info!(
            "Checking for skTerm updates (current: v{})...",
            CURRENT_VERSION
        );
        match fetch_url_text(GITHUB_LATEST_RELEASE_API) {
            Ok(json) => {
                match parse_github_release_json_for_platform(
                    &json,
                    std::env::consts::OS,
                    std::env::consts::ARCH,
                ) {
                    Ok(info) => {
                        if is_newer_version(CURRENT_VERSION, &info.tag_name) {
                            info!(
                                "New skTerm version available: {} (asset: {:?})",
                                info.tag_name, info.asset_name
                            );
                            set_update_status(UpdateStatus::Available(info));
                        } else {
                            info!("skTerm is up to date (v{}).", CURRENT_VERSION);
                            set_update_status(UpdateStatus::UpToDate {
                                version: CURRENT_VERSION.to_string(),
                            });
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse GitHub release metadata: {}", e);
                        set_update_status(UpdateStatus::Failed(e));
                    }
                }
            }
            Err(e) => {
                warn!("Update check failed: {}", e);
                set_update_status(UpdateStatus::Failed(e));
            }
        }
    });
}

/// Spawn a non-blocking background thread to download, verify, and install `info` in-place.
pub fn trigger_download_and_install(info: ReleaseInfo) {
    let Some(asset_url) = info.asset_url.clone() else {
        let _ = crate::app::open_url_in_browser(&info.html_url);
        return;
    };
    let asset_name = info
        .asset_name
        .clone()
        .unwrap_or_else(|| "skTerm-update.tar.gz".to_string());

    set_update_status(UpdateStatus::Downloading {
        info: info.clone(),
        stage: asset_name.clone(),
    });

    std::thread::spawn(move || {
        let work_dir = std::env::temp_dir().join(format!("skterm-ota-{}", std::process::id()));
        let _ = fs::remove_dir_all(&work_dir);
        if let Err(e) = fs::create_dir_all(&work_dir) {
            set_update_status(UpdateStatus::Failed(format!("無法建立暫存目錄: {}", e)));
            return;
        }

        let archive_path = work_dir.join(&asset_name);
        info!("Downloading OTA asset {} -> {:?}", asset_url, archive_path);
        if let Err(e) = download_url_to_file(&asset_url, &archive_path) {
            let _ = fs::remove_dir_all(&work_dir);
            set_update_status(UpdateStatus::Failed(e));
            return;
        }

        // Verify SHA-256 if checksum asset is published on the release
        if let Some(checksum_url) = &info.checksum_url {
            if let Ok(sums_text) = fetch_url_text(checksum_url) {
                if let Some(expected_hash) = parse_sha256sums(&sums_text, &asset_name) {
                    match sha256_file(&archive_path) {
                        Ok(actual_hash) => {
                            if !actual_hash.eq_ignore_ascii_case(&expected_hash) {
                                let _ = fs::remove_dir_all(&work_dir);
                                set_update_status(UpdateStatus::Failed(format!(
                                    "SHA-256 校驗失敗！預期 {}，實際 {}",
                                    &expected_hash[..12],
                                    &actual_hash[..12]
                                )));
                                return;
                            }
                            info!("SHA-256 verified for {}: {}", asset_name, actual_hash);
                        }
                        Err(e) => {
                            let _ = fs::remove_dir_all(&work_dir);
                            set_update_status(UpdateStatus::Failed(format!(
                                "無法計算下載檔案 SHA-256: {}",
                                e
                            )));
                            return;
                        }
                    }
                }
            }
        }

        let install_res = install_downloaded_archive(&archive_path, &asset_name, &work_dir);
        let _ = fs::remove_dir_all(&work_dir);

        match install_res {
            Ok(target_path) => {
                info!(
                    "Successfully installed skTerm v{} to {:?}",
                    info.version, target_path
                );
                set_update_status(UpdateStatus::ReadyToRestart {
                    version: info.version,
                    target_path,
                });
            }
            Err(e) => {
                error!("OTA installation failed: {}", e);
                set_update_status(UpdateStatus::Failed(e));
            }
        }
    });
}

fn find_file_recursive(dir: &Path, target_name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|s| s.to_str()) == Some(target_name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file_recursive(&path, target_name) {
                return Some(found);
            }
        }
    }
    None
}

fn install_downloaded_archive(
    archive_path: &Path,
    asset_name: &str,
    work_dir: &Path,
) -> Result<PathBuf, String> {
    let current_exe =
        std::env::current_exe().map_err(|e| format!("無法取得目前執行檔路徑: {}", e))?;
    let extract_dir = work_dir.join("extracted");
    fs::create_dir_all(&extract_dir).map_err(|e| format!("無法建立解壓縮目錄: {}", e))?;

    let lower = asset_name.to_lowercase();

    #[cfg(target_os = "macos")]
    {
        let target_bundle = resolve_macos_app_bundle_from_exe(&current_exe).unwrap_or_else(|| {
            let apps = PathBuf::from("/Applications/skTerm.app");
            if apps.exists() {
                apps
            } else {
                PathBuf::new()
            }
        });

        if lower.ends_with(".dmg") {
            let mount_point = work_dir.join("dmg_mount");
            fs::create_dir_all(&mount_point).map_err(|e| e.to_string())?;
            let attach = Command::new("hdiutil")
                .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
                .arg(&mount_point)
                .arg(archive_path)
                .status()
                .map_err(|e| format!("無法掛載 DMG: {}", e))?;
            if !attach.success() {
                return Err("掛載 DMG 映像檔失敗".to_string());
            }

            let mounted_app = mount_point.join("skTerm.app");
            let staged_app = extract_dir.join("skTerm.app");
            let copy_res = Command::new("cp")
                .arg("-R")
                .arg(&mounted_app)
                .arg(&staged_app)
                .status();
            let _ = Command::new("hdiutil")
                .args(["detach", "-quiet"])
                .arg(&mount_point)
                .status();

            if !copy_res.is_ok_and(|s| s.success()) {
                return Err("無法從 DMG 複製 skTerm.app".to_string());
            }

            if !target_bundle.as_os_str().is_empty() {
                replace_macos_app_bundle_in_place(&target_bundle, &staged_app)?;
                return Ok(target_bundle);
            } else {
                let new_bin = staged_app.join("Contents/MacOS/skterm");
                replace_executable_in_place(&current_exe, &new_bin)?;
                return Ok(current_exe);
            }
        }

        if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            let status = Command::new("tar")
                .arg("-xzf")
                .arg(archive_path)
                .arg("-C")
                .arg(&extract_dir)
                .status()
                .map_err(|e| format!("無法執行 tar 解壓縮: {}", e))?;
            if !status.success() {
                return Err("解壓縮 tar.gz 更新套件失敗".to_string());
            }

            if let Some(staged_app) = find_file_recursive(&extract_dir, "skTerm.app") {
                if !target_bundle.as_os_str().is_empty() {
                    replace_macos_app_bundle_in_place(&target_bundle, &staged_app)?;
                    return Ok(target_bundle);
                } else {
                    let new_bin = staged_app.join("Contents/MacOS/skterm");
                    replace_executable_in_place(&current_exe, &new_bin)?;
                    return Ok(current_exe);
                }
            }
        }
    }

    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || lower.ends_with(".zip") {
        let status = Command::new("tar")
            .arg("-xf")
            .arg(archive_path)
            .arg("-C")
            .arg(&extract_dir)
            .status()
            .map_err(|e| format!("無法執行 tar 解壓縮: {}", e))?;
        if !status.success() {
            return Err("解壓縮更新檔案失敗".to_string());
        }

        let bin_name = if cfg!(target_os = "windows") {
            "skterm.exe"
        } else {
            "skterm"
        };
        let new_bin = find_file_recursive(&extract_dir, bin_name)
            .ok_or_else(|| format!("更新壓縮檔內找不到執行檔 {}", bin_name))?;
        replace_executable_in_place(&current_exe, &new_bin)?;
        return Ok(current_exe);
    }

    if lower.ends_with(".exe") || !lower.contains('.') {
        replace_executable_in_place(&current_exe, archive_path)?;
        return Ok(current_exe);
    }

    Err(format!("不支援自動安裝此格式 ({})，請手動安裝", asset_name))
}

/// Relaunch `skTerm` from `target_path` (either a `.app` bundle or executable) and exit.
pub fn restart_application(target_path: &Path) {
    #[cfg(target_os = "macos")]
    {
        if target_path.extension().and_then(|s| s.to_str()) == Some("app") {
            let _ = Command::new("open").arg("-n").arg(target_path).spawn();
            std::process::exit(0);
        }
    }
    let exe = if target_path.is_file() {
        target_path.to_path_buf()
    } else {
        std::env::current_exe().unwrap_or_else(|_| target_path.to_path_buf())
    };
    let _ = Command::new(exe).spawn();
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_parse_and_ordering() {
        assert_eq!(
            SemVer::parse("v0.1.0"),
            Some(SemVer {
                major: 0,
                minor: 1,
                patch: 0,
                pre: None
            })
        );
        assert_eq!(
            SemVer::parse("1.2.3-beta.1+build42"),
            Some(SemVer {
                major: 1,
                minor: 2,
                patch: 3,
                pre: Some("beta.1".to_string())
            })
        );

        assert!(is_newer_version("0.1.0", "v0.1.1"));
        assert!(is_newer_version("0.1.0", "v0.2.0"));
        assert!(is_newer_version("0.1.0-rc.1", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "v0.1.0"));
        assert!(!is_newer_version("0.2.0", "v0.1.9"));
        assert!(!is_newer_version("0.2.0", "0.2.0-beta.1"));
    }

    #[test]
    fn test_sha256_fips_180_4_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn test_parse_github_release_json_and_asset_selection() {
        let sample_json = r####"{
            "url": "https://api.github.com/repos/ericsk/skTerm/releases/1",
            "html_url": "https://github.com/ericsk/skTerm/releases/tag/v0.2.0",
            "tag_name": "v0.2.0",
            "name": "skTerm v0.2.0",
            "body": "### Features\n- Simplified Chinese support\n- OTA Updater",
            "assets": [
                {
                    "name": "skTerm-v0.2.0-macos-arm64.app.tar.gz",
                    "browser_download_url": "https://github.com/ericsk/skTerm/releases/download/v0.2.0/skTerm-v0.2.0-macos-arm64.app.tar.gz"
                },
                {
                    "name": "skTerm-v0.2.0-macos-arm64.dmg",
                    "browser_download_url": "https://github.com/ericsk/skTerm/releases/download/v0.2.0/skTerm-v0.2.0-macos-arm64.dmg"
                },
                {
                    "name": "skterm-v0.2.0-linux-x86_64.tar.gz",
                    "browser_download_url": "https://github.com/ericsk/skTerm/releases/download/v0.2.0/skterm-v0.2.0-linux-x86_64.tar.gz"
                },
                {
                    "name": "skterm-v0.2.0-windows-x64.zip",
                    "browser_download_url": "https://github.com/ericsk/skTerm/releases/download/v0.2.0/skterm-v0.2.0-windows-x64.zip"
                },
                {
                    "name": "SHA256SUMS.txt",
                    "browser_download_url": "https://github.com/ericsk/skTerm/releases/download/v0.2.0/SHA256SUMS.txt"
                }
            ]
        }"####;

        let mac_info =
            parse_github_release_json_for_platform(sample_json, "macos", "aarch64").unwrap();
        assert_eq!(mac_info.tag_name, "v0.2.0");
        assert_eq!(mac_info.version, "0.2.0");
        assert_eq!(
            mac_info.asset_name.as_deref(),
            Some("skTerm-v0.2.0-macos-arm64.app.tar.gz")
        );
        assert_eq!(
            mac_info.checksum_url.as_deref(),
            Some("https://github.com/ericsk/skTerm/releases/download/v0.2.0/SHA256SUMS.txt")
        );
        assert!(mac_info.body.contains("Simplified Chinese support"));

        let win_info =
            parse_github_release_json_for_platform(sample_json, "windows", "x86_64").unwrap();
        assert_eq!(
            win_info.asset_name.as_deref(),
            Some("skterm-v0.2.0-windows-x64.zip")
        );

        let linux_info =
            parse_github_release_json_for_platform(sample_json, "linux", "x86_64").unwrap();
        assert_eq!(
            linux_info.asset_name.as_deref(),
            Some("skterm-v0.2.0-linux-x86_64.tar.gz")
        );
    }

    #[test]
    fn test_parse_sha256sums_formats() {
        let sums = "\
8e8ba93cb17f040322217c01cd73dedc14565e6f23385ff803ec5fd6430192fd  dist/macos/skTerm-v0.1.0-macos-arm64.dmg
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad *skTerm-v0.1.0-macos-arm64.app.tar.gz
";
        assert_eq!(
            parse_sha256sums(sums, "skTerm-v0.1.0-macos-arm64.dmg").as_deref(),
            Some("8e8ba93cb17f040322217c01cd73dedc14565e6f23385ff803ec5fd6430192fd")
        );
        assert_eq!(
            parse_sha256sums(sums, "skTerm-v0.1.0-macos-arm64.app.tar.gz").as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(parse_sha256sums(sums, "nonexistent.zip"), None);
    }

    #[test]
    fn test_resolve_macos_app_bundle_and_in_place_replace() {
        let exe = Path::new("/Applications/skTerm.app/Contents/MacOS/skterm");
        assert_eq!(
            resolve_macos_app_bundle_from_exe(exe),
            Some(PathBuf::from("/Applications/skTerm.app"))
        );
        assert_eq!(
            resolve_macos_app_bundle_from_exe(Path::new("/usr/local/bin/skterm")),
            None
        );

        let temp = std::env::temp_dir().join(format!("skterm-test-replace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let target_bin = temp.join("skterm");
        let new_bin = temp.join("skterm_new");
        fs::write(&target_bin, b"old-binary-v1").unwrap();
        fs::write(&new_bin, b"new-binary-v2").unwrap();

        replace_executable_in_place(&target_bin, &new_bin).unwrap();
        assert_eq!(fs::read(&target_bin).unwrap(), b"new-binary-v2");

        let _ = fs::remove_dir_all(&temp);
    }
}
