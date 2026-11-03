use std::path::Path;

use assert_cmd::Command;

/// Keep CLI tests away from the caller's profiles, sessions, and API endpoints.
pub fn isolated_command(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("sharepoint").unwrap();
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if name.starts_with("SHAREPOINT_") || name.starts_with("MICROSOFT_") {
            cmd.env_remove(key);
        }
    }
    // Preserve the OS environment (notably SystemRoot on Windows).
    cmd.env("HOME", root)
        .env("XDG_CONFIG_HOME", root)
        .env("XDG_CACHE_HOME", root);
    cmd
}
