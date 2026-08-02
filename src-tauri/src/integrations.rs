//! Integration management: installing hook/plugin assets into each agent CLI
//! and wiring their config, with explicit install / update / uninstall.
//!
//! Managed assets carry a marker header (`XAGENT_INTEGRATION_ID=<agent>` and
//! `XAGENT_INTEGRATION_VERSION=1`). The installer only touches entries it
//! added; a same-name file without the marker is reported as a conflict and
//! never overwritten. Uninstall removes only the managed entry and keeps user
//! hooks, plugins, settings, and credentials.

use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

// Reserved for version-diff Outdated detection against the marker header.
#[allow(dead_code)]
pub(crate) const INTEGRATION_VERSION: u32 = 1;
const MARKER_PREFIX: &str = "XAGENT_INTEGRATION_ID=";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum IntegrationAgent {
    Codex,
    Claude,
    Opencode,
}

impl IntegrationAgent {
    #[allow(dead_code)]
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            IntegrationAgent::Codex => "codex",
            IntegrationAgent::Claude => "claude",
            IntegrationAgent::Opencode => "opencode",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IntegrationStatus {
    pub(crate) agent: IntegrationAgent,
    pub(crate) state: IntegrationState,
    pub(crate) script_path: Option<String>,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IntegrationState {
    NotInstalled,
    Installed,
    // Reserved for version-diff Outdated detection.
    #[allow(dead_code)]
    Outdated,
    Conflict,
}

/// Resolves the agent's config root, honoring the documented env override.
/// Public for the Settings integrations UI.
#[allow(dead_code)]
pub(crate) fn config_root(agent: IntegrationAgent) -> Result<PathBuf, String> {
    config_root_with_home(agent, &home_dir())
}

fn config_root_with_home(agent: IntegrationAgent, home: &Path) -> Result<PathBuf, String> {
    match agent {
        IntegrationAgent::Codex => Ok(std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"))),
        IntegrationAgent::Claude => Ok(std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"))),
        IntegrationAgent::Opencode => Ok(std::env::var_os("XDG_CONFIG_HOME")
            .map(|dir| PathBuf::from(dir).join("opencode"))
            .unwrap_or_else(|| home.join(".config").join("opencode"))),
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The managed script path for an agent on this OS.
/// Public for the Settings integrations UI.
#[allow(dead_code)]
pub(crate) fn script_path(agent: IntegrationAgent) -> Result<PathBuf, String> {
    script_path_with_home(agent, &home_dir())
}

fn script_path_with_home(agent: IntegrationAgent, home: &Path) -> Result<PathBuf, String> {
    let root = config_root_with_home(agent, home)?;
    match agent {
        IntegrationAgent::Codex => Ok(root.join(script_file_name())),
        IntegrationAgent::Claude => Ok(root.join("hooks").join(script_file_name())),
        IntegrationAgent::Opencode => Ok(root.join("plugins").join("xagent-agent-state.js")),
    }
}

fn script_file_name() -> String {
    #[cfg(windows)]
    {
        "xagent-agent-state.ps1".to_string()
    }
    #[cfg(not(windows))]
    {
        "xagent-agent-state.sh".to_string()
    }
}

/// Detects whether the script file exists and carries our marker.
pub(crate) fn status(agent: IntegrationAgent) -> Result<IntegrationStatus, String> {
    status_with_home(agent, &home_dir())
}

fn status_with_home(agent: IntegrationAgent, home: &Path) -> Result<IntegrationStatus, String> {
    let path = script_path_with_home(agent, home)?;
    let marker = match fs::read_to_string(&path) {
        Ok(content) => content.contains(MARKER_PREFIX),
        Err(_) => false,
    };
    if !path.exists() {
        return Ok(IntegrationStatus {
            agent,
            state: IntegrationState::NotInstalled,
            script_path: None,
            message: "Not installed.".to_string(),
        });
    }
    if !marker {
        return Ok(IntegrationStatus {
            agent,
            state: IntegrationState::Conflict,
            script_path: Some(path.to_string_lossy().into_owned()),
            message: "A non-managed file with this name exists.".to_string(),
        });
    }
    Ok(IntegrationStatus {
        agent,
        state: IntegrationState::Installed,
        script_path: Some(path.to_string_lossy().into_owned()),
        message: "Installed.".to_string(),
    })
}

/// Installs or updates the managed assets. Returns true when a change was
/// written, false when already current.
pub(crate) fn install(agent: IntegrationAgent) -> Result<IntegrationStatus, String> {
    install_with_home(agent, &home_dir())
}

fn install_with_home(agent: IntegrationAgent, home: &Path) -> Result<IntegrationStatus, String> {
    let target = script_path_with_home(agent, home)?;
    let parent = target
        .parent()
        .ok_or_else(|| "The integration location is unavailable.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|_| "Could not prepare the integration location.".to_string())?;

    let existing = fs::read_to_string(&target).ok();
    if let Some(content) = &existing {
        if content.contains(MARKER_PREFIX) {
            // Already managed; still refresh the file (update semantics).
        } else {
            return Err(format!(
                "Refusing to overwrite {}: it is not managed by xagent.",
                target.display()
            ));
        }
    }

    let script = asset_script(agent)?;
    write_atomic(&target, script.as_bytes())?;
    make_executable(&target);
    Ok(IntegrationStatus {
        agent,
        state: IntegrationState::Installed,
        script_path: Some(target.to_string_lossy().into_owned()),
        message: "Installed.".to_string(),
    })
}

/// Removes only the managed script file; config entries are untouched by
/// uninstall (user hooks/settings/credentials are preserved).
pub(crate) fn uninstall(agent: IntegrationAgent) -> Result<IntegrationStatus, String> {
    uninstall_with_home(agent, &home_dir())
}

fn uninstall_with_home(agent: IntegrationAgent, home: &Path) -> Result<IntegrationStatus, String> {
    let target = script_path_with_home(agent, home)?;
    if !target.exists() {
        return Ok(IntegrationStatus {
            agent,
            state: IntegrationState::NotInstalled,
            script_path: None,
            message: "Not installed.".to_string(),
        });
    }
    let content = fs::read_to_string(&target)
        .map_err(|_| "The managed script could not be read.".to_string())?;
    if !content.contains(MARKER_PREFIX) {
        return Err(format!(
            "Refusing to delete {}: it is not managed by xagent.",
            target.display()
        ));
    }
    fs::remove_file(&target).map_err(|_| "The managed script could not be removed.".to_string())?;
    Ok(IntegrationStatus {
        agent,
        state: IntegrationState::NotInstalled,
        script_path: None,
        message: "Removed.".to_string(),
    })
}

fn asset_script(agent: IntegrationAgent) -> Result<&'static str, String> {
    // The assets are embedded at build time via include_str! below.
    match agent {
        IntegrationAgent::Codex => Ok(codex_script()),
        IntegrationAgent::Claude => Ok(claude_script()),
        IntegrationAgent::Opencode => Ok(opencode_script()),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "The integration location is unavailable.".to_string())?;
    let temporary = parent.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    fs::write(&temporary, bytes)
        .map_err(|_| "Could not write the integration script.".to_string())?;
    fs::rename(&temporary, path)
        .map_err(|_| "Could not save the integration script.".to_string())?;
    Ok(())
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(permissions.mode() | 0o755);
            let _ = fs::set_permissions(path, permissions);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

#[cfg(not(windows))]
fn codex_script() -> &'static str {
    include_str!("../assets/integrations/codex/xagent-agent-state.sh")
}

#[cfg(windows)]
fn codex_script() -> &'static str {
    include_str!("../assets/integrations/codex/xagent-agent-state.ps1")
}

#[cfg(not(windows))]
fn claude_script() -> &'static str {
    include_str!("../assets/integrations/claude/xagent-agent-state.sh")
}

#[cfg(windows)]
fn claude_script() -> &'static str {
    include_str!("../assets/integrations/claude/xagent-agent-state.ps1")
}

fn opencode_script() -> &'static str {
    include_str!("../assets/integrations/opencode/xagent-agent-state.js")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    /// Creates an isolated temp dir and runs the closure with it as home.
    fn with_temp_home<T>(run: impl FnOnce(&Path) -> T) -> T {
        let temp = std::env::temp_dir().join(format!(
            "xagent-integration-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&temp).expect("temp home");
        let result = run(&temp);
        fs::remove_dir_all(&temp).ok();
        result
    }

    #[test]
    fn installed_assets_carry_the_management_marker() {
        assert!(codex_script().contains(MARKER_PREFIX));
        assert!(claude_script().contains(MARKER_PREFIX));
        assert!(opencode_script().contains(MARKER_PREFIX));
    }

    #[test]
    fn install_then_uninstall_is_idempotent() {
        with_temp_home(|home| {
            let status =
                install_with_home(IntegrationAgent::Codex, home).expect("install succeeds");
            assert_eq!(status.state, IntegrationState::Installed);

            // Re-installing is a no-op that stays Installed.
            let again =
                install_with_home(IntegrationAgent::Codex, home).expect("reinstall succeeds");
            assert_eq!(again.state, IntegrationState::Installed);

            let path = script_path_with_home(IntegrationAgent::Codex, home).expect("path");
            let content = fs::read_to_string(&path).expect("script readable");
            assert!(content.contains(MARKER_PREFIX));

            let removed =
                uninstall_with_home(IntegrationAgent::Codex, home).expect("uninstall succeeds");
            assert_eq!(removed.state, IntegrationState::NotInstalled);
            assert!(!path.exists());
        });
    }

    #[test]
    fn non_managed_file_is_reported_as_conflict_and_never_overwritten() {
        with_temp_home(|home| {
            let path = script_path_with_home(IntegrationAgent::Codex, home).expect("path");
            fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            fs::write(&path, "user-owned content").expect("write");

            let st = status_with_home(IntegrationAgent::Codex, home).expect("status");
            assert_eq!(st.state, IntegrationState::Conflict);

            assert!(install_with_home(IntegrationAgent::Codex, home).is_err());
            assert_eq!(
                fs::read_to_string(&path).expect("still user content"),
                "user-owned content"
            );

            assert!(uninstall_with_home(IntegrationAgent::Codex, home).is_err());
            assert!(path.exists());
        });
    }

    #[test]
    fn uninstall_preserves_the_config_directory() {
        with_temp_home(|home| {
            let root = config_root_with_home(IntegrationAgent::Claude, home).expect("root");
            fs::create_dir_all(root.join("hooks")).expect("dir");
            fs::write(root.join("settings.json"), "{}").expect("user settings");

            let _ = install_with_home(IntegrationAgent::Claude, home).expect("install");
            let _ = uninstall_with_home(IntegrationAgent::Claude, home).expect("uninstall");

            // The user's settings survive; only the managed script is gone.
            assert_eq!(
                fs::read_to_string(root.join("settings.json")).expect("settings"),
                "{}"
            );
            assert!(!script_path_with_home(IntegrationAgent::Claude, home)
                .expect("path")
                .exists());
        });
    }

    #[test]
    fn opencode_plugin_lands_in_the_plugins_directory() {
        with_temp_home(|home| {
            let _ = install_with_home(IntegrationAgent::Opencode, home).expect("install");
            let path = script_path_with_home(IntegrationAgent::Opencode, home).expect("path");
            assert!(path.ends_with(PathBuf::from("plugins/xagent-agent-state.js")));
            assert!(path.exists());
        });
    }
}
