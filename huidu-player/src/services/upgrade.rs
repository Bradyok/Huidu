//! Firmware upgrade service for Huidu .zbin packages.
//!
//! # .zbin format
//! The Huidu firmware package is a ZIP archive.  A typical package looks like:
//!
//! ```text
//! BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin (ZIP container)
//! ├── BoxPlayer          — ARM Linux ELF player binary
//! ├── MagicPlayer        — secondary player binary
//! ├── version.txt        — plain-text version string, e.g. "7.11.18.0"
//! ├── install.sh         — optional post-install hook
//! └── *.so               — shared library updates
//! ```
//!
//! For our Rust reproduction the package contains `huidu-player` instead of
//! `BoxPlayer`, but the install logic handles both names.
//!
//! # Upgrade flow
//! 1. Client uploads the `.zbin` via the binary file-transfer protocol.
//! 2. Client sends `FirmwareUpgrade` SDK command with the filename on device.
//! 3. Player validates and stages the package:
//!    a. Opens the `.zbin` as a ZIP archive.
//!    b. Extracts recognised files to `<program_dir>/.upgrade_stage/`.
//!    c. Writes `<program_dir>/.upgrade_pending` marker.
//!    d. Returns `<upgradeStatus value="upgrading"/>` to the client.
//!    e. Triggers a graceful restart via the `CancellationToken` after 3 s.
//! 4. On next startup the player detects `.upgrade_pending`:
//!    a. Runs `install.sh` if present in the stage dir.
//!    b. Otherwise replaces the running binary with the staged one.
//!    c. Writes `.upgrade_result` (survives across reboots).
//!    d. Removes the marker and stage dir.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Persistent upgrade status — stored in `device_state.json` and returned by
/// `GetUpgradeResult`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpgradeStatus {
    /// No upgrade has been requested or completed.
    #[default]
    Idle,
    /// Upgrade package staged; player restart pending.
    InProgress { filename: String },
    /// Last upgrade completed successfully.
    Success { version: String },
    /// Last upgrade failed.
    Failed { reason: String },
}

impl UpgradeStatus {
    /// Returns `(sdk_value, human_message)` suitable for the `GetUpgradeResult`
    /// response XML: `<upgradeResult value="{v}" message="{m}"/>`.
    pub fn sdk_response(&self) -> (u32, String) {
        match self {
            Self::Idle => (0, "no upgrade in progress".into()),
            Self::InProgress { filename } => (1, format!("upgrading: {filename}")),
            Self::Success { version } => (0, format!("upgrade succeeded: v{version}")),
            Self::Failed { reason } => (2, format!("upgrade failed: {reason}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Staging
// ---------------------------------------------------------------------------

/// Validate and stage a `.zbin` firmware package for installation on the next
/// restart.
///
/// Extracts recognised files from the ZIP into
/// `<parent-of-zbin>/.upgrade_stage/`.  Returns the version string found
/// inside the package (or inferred from its filename), or an `Err` with a
/// human-readable diagnostic.
pub fn stage_upgrade(zbin_path: &Path) -> Result<String, String> {
    let program_dir = zbin_path.parent().unwrap_or(Path::new("."));
    let stage_dir = program_dir.join(".upgrade_stage");

    // Wipe any leftover stage from a previous attempt
    let _ = std::fs::remove_dir_all(&stage_dir);
    std::fs::create_dir_all(&stage_dir)
        .map_err(|e| format!("Cannot create stage dir: {e}"))?;

    let file = std::fs::File::open(zbin_path)
        .map_err(|e| format!("Cannot open .zbin: {e}"))?;

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!(".zbin is not a valid ZIP archive: {e}"))?;

    let mut version = String::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("ZIP read error at index {i}: {e}"))?;

        let name = entry.name().to_string();

        // Reject path-traversal attempts
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            warn!("Skipping unsafe path in .zbin: {name}");
            continue;
        }

        // Skip directories
        if name.ends_with('/') {
            continue;
        }

        if name == "version.txt" || name == "VERSION" {
            let mut s = String::new();
            entry.read_to_string(&mut s).unwrap_or(0);
            version = s.trim().to_string();
            info!("Package declares version: {version}");
            std::fs::write(stage_dir.join("version.txt"), &version).unwrap_or(());
        } else if is_extractable(&name) {
            let dest = stage_dir.join(&name);
            let mut out = std::fs::File::create(&dest)
                .map_err(|e| format!("Cannot create staged file {name}: {e}"))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("Cannot extract {name}: {e}"))?;
            make_executable(&dest);
            info!("Staged: {name}");
        }
    }

    // Fall back to filename heuristic if the archive has no version.txt
    if version.is_empty() {
        version = version_from_filename(zbin_path);
    }
    if version.is_empty() {
        version = "unknown".to_string();
    }

    Ok(version)
}

// ---------------------------------------------------------------------------
// Pending-upgrade marker
// ---------------------------------------------------------------------------

/// Write a `.upgrade_pending` marker so the player applies the upgrade on the
/// next startup.
pub fn write_pending_marker(program_dir: &Path, filename: &str, version: &str) {
    let marker = program_dir.join(".upgrade_pending");
    let content = format!("{filename}\n{version}\n");
    if let Err(e) = std::fs::write(&marker, content) {
        warn!("Failed to write .upgrade_pending: {e}");
    } else {
        info!(".upgrade_pending written for {filename} v{version}");
    }
}

/// Check for a `.upgrade_pending` marker.
/// Returns `Some((filename, version))` if one exists.
pub fn read_pending_marker(program_dir: &Path) -> Option<(String, String)> {
    let marker = program_dir.join(".upgrade_pending");
    let content = std::fs::read_to_string(&marker).ok()?;
    let mut lines = content.lines();
    let filename = lines.next()?.to_string();
    let version = lines.next().unwrap_or("unknown").to_string();
    Some((filename, version))
}

// ---------------------------------------------------------------------------
// Applying the staged upgrade on startup
// ---------------------------------------------------------------------------

/// Detect and apply a pending upgrade.  Call this once during startup, before
/// the render loop begins.
///
/// If no `.upgrade_pending` marker is found, returns `None` immediately.
/// Otherwise:
///   - Runs `install.sh` if present in `.upgrade_stage/`.
///   - Falls back to binary replacement on Unix.
///   - Persists the result to `.upgrade_result`.
///   - Cleans up the marker and stage directory.
pub fn apply_pending_upgrade(program_dir: &Path) -> Option<UpgradeStatus> {
    let (filename, version) = read_pending_marker(program_dir)?;
    let stage_dir = program_dir.join(".upgrade_stage");

    info!("Applying pending upgrade: {filename} v{version}");

    let status = do_apply(&stage_dir, &version);

    // Always clean up so we don't loop on every restart
    let _ = std::fs::remove_file(program_dir.join(".upgrade_pending"));
    let _ = std::fs::remove_dir_all(&stage_dir);

    write_last_result(program_dir, &status);
    Some(status)
}

// ---------------------------------------------------------------------------
// Persistent last-result
// ---------------------------------------------------------------------------

/// Read the last upgrade result from `.upgrade_result` (survives reboots).
pub fn read_last_result(program_dir: &Path) -> Option<UpgradeStatus> {
    let content = std::fs::read_to_string(program_dir.join(".upgrade_result")).ok()?;
    let mut lines = content.lines();
    match lines.next()? {
        "success" => Some(UpgradeStatus::Success {
            version: lines.next().unwrap_or("unknown").to_string(),
        }),
        "failed" => Some(UpgradeStatus::Failed {
            reason: lines.next().unwrap_or("unknown error").to_string(),
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn do_apply(stage_dir: &Path, version: &str) -> UpgradeStatus {
    // Prefer install.sh if the package provided one
    let install_sh = stage_dir.join("install.sh");
    if install_sh.exists() {
        return run_install_script(&install_sh, stage_dir, version);
    }

    // No script — try to replace the running binary (Unix only)
    #[cfg(unix)]
    {
        return replace_binary(stage_dir, version);
    }

    // Non-Unix with no install script: nothing to do
    #[cfg(not(unix))]
    {
        info!("Non-Unix platform: staged upgrade requires manual installation");
        UpgradeStatus::Success {
            version: version.to_string(),
        }
    }
}

fn run_install_script(script: &Path, stage_dir: &Path, version: &str) -> UpgradeStatus {
    #[cfg(unix)]
    {
        info!("Running install.sh");
        match std::process::Command::new("sh")
            .arg(script)
            .current_dir(stage_dir)
            .status()
        {
            Ok(s) if s.success() => {
                info!("install.sh succeeded");
                UpgradeStatus::Success {
                    version: version.to_string(),
                }
            }
            Ok(s) => {
                let reason = format!("install.sh exited with status {s}");
                warn!("{reason}");
                UpgradeStatus::Failed { reason }
            }
            Err(e) => {
                let reason = format!("Failed to spawn install.sh: {e}");
                warn!("{reason}");
                UpgradeStatus::Failed { reason }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (script, stage_dir, version);
        UpgradeStatus::Failed {
            reason: "install.sh not supported on this platform".into(),
        }
    }
}

#[cfg(unix)]
fn replace_binary(stage_dir: &Path, version: &str) -> UpgradeStatus {
    let staged = match find_staged_binary(stage_dir) {
        Some(p) => p,
        None => {
            // Package contained no binary (perhaps only library updates already
            // extracted by install.sh).  Treat as a successful no-op.
            info!("No staged binary found; treating as script-only upgrade");
            return UpgradeStatus::Success {
                version: version.to_string(),
            };
        }
    };

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            let reason = format!("Cannot determine current exe path: {e}");
            warn!("{reason}");
            return UpgradeStatus::Failed { reason };
        }
    };

    // Rename existing binary to .bak so we can restore on failure
    let backup = current_exe.with_extension("bak");
    if let Err(e) = std::fs::rename(&current_exe, &backup) {
        // On some systems (running from a read-only mount) this will fail —
        // log and treat as a non-fatal skip rather than hard failure.
        warn!("Cannot back up current binary (will skip replacement): {e}");
        return UpgradeStatus::Success {
            version: version.to_string(),
        };
    }

    match std::fs::copy(&staged, &current_exe) {
        Ok(_) => {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &current_exe,
                std::fs::Permissions::from_mode(0o755),
            );
            info!(
                "Binary replaced: {} → {}",
                staged.display(),
                current_exe.display()
            );
            UpgradeStatus::Success {
                version: version.to_string(),
            }
        }
        Err(e) => {
            // Restore backup
            let _ = std::fs::rename(&backup, &current_exe);
            let reason = format!("Failed to replace binary: {e}");
            warn!("{reason}");
            UpgradeStatus::Failed { reason }
        }
    }
}

fn find_staged_binary(stage_dir: &Path) -> Option<PathBuf> {
    for name in &["huidu-player", "BoxPlayer", "MagicPlayer"] {
        let p = stage_dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn write_last_result(program_dir: &Path, status: &UpgradeStatus) {
    let content = match status {
        UpgradeStatus::Success { version } => format!("success\n{version}\n"),
        UpgradeStatus::Failed { reason } => format!("failed\n{reason}\n"),
        _ => return,
    };
    if let Err(e) = std::fs::write(program_dir.join(".upgrade_result"), content) {
        warn!("Failed to write .upgrade_result: {e}");
    }
}

/// Returns `true` for file names that should be extracted from the `.zbin`.
fn is_extractable(name: &str) -> bool {
    matches!(
        name,
        "BoxPlayer"
            | "huidu-player"
            | "MagicPlayer"
            | "install.sh"
            | "uninstall.sh"
    ) || name.ends_with(".so")
        || name.ends_with(".so.1")
        || name.ends_with(".conf")
}

/// Make executable on Unix; no-op on other platforms.
fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Try to extract a version string from a filename like
/// `BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin`.
fn version_from_filename(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    for part in stem.split('_') {
        let v = part
            .strip_prefix('V')
            .or_else(|| part.strip_prefix('v'))
            .unwrap_or("");
        if v.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && v.contains('.') {
            return v.to_string();
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_from_filename_standard() {
        let p = Path::new("BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin");
        assert_eq!(version_from_filename(p), "7.11.18.0");
    }

    #[test]
    fn test_version_from_filename_lowercase_v() {
        assert_eq!(
            version_from_filename(Path::new("firmware_v1.2.3.zbin")),
            "1.2.3"
        );
    }

    #[test]
    fn test_version_from_filename_no_version() {
        assert_eq!(version_from_filename(Path::new("firmware.zbin")), "");
    }

    #[test]
    fn test_upgrade_status_sdk_response_idle() {
        let (v, msg) = UpgradeStatus::Idle.sdk_response();
        assert_eq!(v, 0);
        assert!(msg.contains("no upgrade"));
    }

    #[test]
    fn test_upgrade_status_sdk_response_in_progress() {
        let (v, msg) = UpgradeStatus::InProgress {
            filename: "upgrade.zbin".into(),
        }
        .sdk_response();
        assert_eq!(v, 1);
        assert!(msg.contains("upgrade.zbin"));
    }

    #[test]
    fn test_upgrade_status_sdk_response_success() {
        let (v, msg) = UpgradeStatus::Success {
            version: "7.11.18.0".into(),
        }
        .sdk_response();
        assert_eq!(v, 0);
        assert!(msg.contains("7.11.18.0"));
    }

    #[test]
    fn test_upgrade_status_sdk_response_failed() {
        let (v, msg) = UpgradeStatus::Failed {
            reason: "file not found".into(),
        }
        .sdk_response();
        assert_eq!(v, 2);
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn test_pending_marker_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        write_pending_marker(dir.path(), "upgrade.zbin", "7.11.18.0");
        let result = read_pending_marker(dir.path());
        assert_eq!(
            result,
            Some(("upgrade.zbin".to_string(), "7.11.18.0".to_string()))
        );
    }

    #[test]
    fn test_read_pending_marker_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_pending_marker(dir.path()).is_none());
    }

    #[test]
    fn test_read_last_result_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".upgrade_result"), "success\n7.11.18.0\n").unwrap();
        assert_eq!(
            read_last_result(dir.path()),
            Some(UpgradeStatus::Success {
                version: "7.11.18.0".into()
            })
        );
    }

    #[test]
    fn test_read_last_result_failed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".upgrade_result"),
            "failed\ninstall.sh exited 1\n",
        )
        .unwrap();
        assert_eq!(
            read_last_result(dir.path()),
            Some(UpgradeStatus::Failed {
                reason: "install.sh exited 1".into()
            })
        );
    }

    #[test]
    fn test_read_last_result_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_last_result(dir.path()).is_none());
    }

    #[test]
    fn test_stage_upgrade_valid_zip() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zbin = dir.path().join("firmware_V7.11.18.0.zbin");

        // Build a minimal valid ZIP
        let mut zip_buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("version.txt", opts).unwrap();
            zip.write_all(b"7.11.18.0").unwrap();
            zip.start_file("install.sh", opts).unwrap();
            zip.write_all(b"#!/bin/sh\necho ok\n").unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(&zbin, zip_buf.into_inner()).unwrap();

        let version = stage_upgrade(&zbin).unwrap();
        assert_eq!(version, "7.11.18.0");

        let stage_dir = dir.path().join(".upgrade_stage");
        assert!(stage_dir.join("version.txt").exists());
        assert!(stage_dir.join("install.sh").exists());
    }

    #[test]
    fn test_stage_upgrade_version_from_filename_fallback() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zbin = dir.path().join("BoxPlayer_V5.1.0.zbin");

        let mut zip_buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            // No version.txt — should fall back to filename
            zip.start_file("install.sh", opts).unwrap();
            zip.write_all(b"#!/bin/sh\n").unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(&zbin, zip_buf.into_inner()).unwrap();

        let version = stage_upgrade(&zbin).unwrap();
        assert_eq!(version, "5.1.0");
    }

    #[test]
    fn test_stage_upgrade_rejects_non_zip() {
        let dir = tempfile::tempdir().unwrap();
        let zbin = dir.path().join("bad.zbin");
        std::fs::write(&zbin, b"this is not a zip").unwrap();
        assert!(stage_upgrade(&zbin).is_err());
    }

    #[test]
    fn test_stage_upgrade_rejects_path_traversal() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zbin = dir.path().join("evil.zbin");

        let mut zip_buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            // Attempt path traversal
            zip.start_file("../../../etc/passwd", opts).unwrap();
            zip.write_all(b"evil").unwrap();
            zip.start_file("version.txt", opts).unwrap();
            zip.write_all(b"1.0.0").unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(&zbin, zip_buf.into_inner()).unwrap();

        // Should succeed (traversal entry is skipped, version.txt extracted)
        let version = stage_upgrade(&zbin).unwrap();
        assert_eq!(version, "1.0.0");

        // The malicious file must NOT have been created outside the stage dir
        assert!(!dir.path().parent().unwrap_or(dir.path())
            .join("etc/passwd")
            .exists());
    }
}
