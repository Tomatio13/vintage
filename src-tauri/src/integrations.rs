//! Integration management: installing hook/plugin assets into each agent CLI
//! and wiring their config, with explicit install / update / uninstall.
//!
//! Managed assets carry a marker header (`VINTAGE_INTEGRATION_ID=<agent>` and
//! `VINTAGE_INTEGRATION_VERSION=3`). The installer only touches entries it
//! added; a same-name file without the marker is reported as a conflict and
//! never overwritten. Uninstall removes only the managed entry and keeps user
//! hooks, plugins, settings, and credentials.

use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::shells::decode_powershell_script;
#[cfg(windows)]
use crate::shells::encode_powershell_script;
use serde_json::Value;

// Reserved for version-diff Outdated detection against the marker header.
#[allow(dead_code)]
pub(crate) const INTEGRATION_VERSION: u32 = 4;
const MARKER_PREFIX: &str = "VINTAGE_INTEGRATION_ID=";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum IntegrationAgent {
    Codex,
    Claude,
    Opencode,
}

impl IntegrationAgent {
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
    // Prefer the OS home, then the usual env vars. Falling back to "." makes
    // the app look for configs in its own working directory, which is why
    // installed integrations showed as missing in some launches.
    #[allow(deprecated)]
    std::env::home_dir()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
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
        IntegrationAgent::Opencode => Ok(root.join("plugins").join("vintage-agent-state.js")),
    }
}

fn script_file_name() -> String {
    #[cfg(windows)]
    {
        "vintage-agent-state.ps1".to_string()
    }
    #[cfg(not(windows))]
    {
        "vintage-agent-state.sh".to_string()
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
    // A script alone is not enough: the SessionStart entry must also be wired
    // for the CLI to invoke it. A managed script with a missing entry is
    // reported as NotInstalled so the Integrations screen offers Install.
    let config_wired = is_config_wired(agent, home)?;
    if !config_wired {
        return Ok(IntegrationStatus {
            agent,
            state: IntegrationState::NotInstalled,
            script_path: Some(path.to_string_lossy().into_owned()),
            message: "The managed script is present but not wired to SessionStart.".to_string(),
        });
    }
    Ok(IntegrationStatus {
        agent,
        state: IntegrationState::Installed,
        script_path: Some(path.to_string_lossy().into_owned()),
        message: "Installed.".to_string(),
    })
}

/// True when the agent's config carries the managed SessionStart entry.
fn is_config_wired(agent: IntegrationAgent, home: &Path) -> Result<bool, String> {
    if agent == IntegrationAgent::Opencode {
        return Ok(true); // Plugin auto-loads from the plugins dir; no wiring.
    }
    let path = config_path(agent, home)?;
    let Ok(doc) = read_config_json(&path) else {
        return Ok(false);
    };
    let command = managed_hook_command(agent, home, "session");
    let Some(hooks) = doc.get("hooks").and_then(Value::as_object) else {
        return Ok(false);
    };
    let Some(session_start) = hooks.get("SessionStart").and_then(Value::as_array) else {
        return Ok(false);
    };
    Ok(session_start
        .iter()
        .any(|entry| entry_contains_marker(entry, &command)))
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
                "Refusing to overwrite {}: it is not managed by VINTAGE.",
                target.display()
            ));
        }
    }

    let script = asset_script(agent)?;
    write_atomic(&target, script.as_bytes())?;
    make_executable(&target);
    // Wire the SessionStart config entry so the CLI actually invokes the hook.
    wire_config(agent, home)?;
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
            "Refusing to delete {}: it is not managed by VINTAGE.",
            target.display()
        ));
    }
    fs::remove_file(&target).map_err(|_| "The managed script could not be removed.".to_string())?;
    unwire_config(agent, home)?;
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

// ---------------------------------------------------------------------------
// Config wiring: hooks.json (Codex) / settings.json (Claude) SessionStart
// entries. The managed command embeds a marker so we can find and remove only
// the entry we added, leaving user hooks and settings untouched.
// ---------------------------------------------------------------------------

/// The managed hook command for an agent, with a marker embedded in a
/// comment so the exact entry can be located on uninstall. The command uses
/// the shell's absolute path and carries a per-agent marker
/// (`# vintage:codex`) so each integration is recognizable and removable
/// without touching user hooks.
///
/// `action` selects the script behavior (session / idle / working / blocked);
/// the marker appends it so each event's entry is individually removable.
fn managed_hook_command(agent: IntegrationAgent, home: &Path, action: &str) -> String {
    let script = script_file_name();
    let root = config_root_with_home(agent, home).unwrap_or_default();
    let marker = format!("{}:{}:{}", CONFIG_MARKER, agent.as_str(), action);
    let script_path = match agent {
        IntegrationAgent::Codex => root.join(&script),
        IntegrationAgent::Claude => root.join("hooks").join(&script),
        IntegrationAgent::Opencode => return String::new(),
    };
    hook_command(&script_path, action, &marker)
}

/// The managed hook command that runs `script_path` with `action`. On Unix the
/// launcher takes the script path and action as shell words; on Windows the
/// whole invocation is base64-encoded (`-EncodedCommand`) so quoting cannot be
/// mangled by the agent's hook shell — cmd.exe for Codex, Git Bash or
/// PowerShell for Claude Code.
fn hook_command(script_path: &Path, action: &str, marker: &str) -> String {
    #[cfg(windows)]
    {
        let payload = format!("& '{}' '{}'  {}", script_path.display(), action, marker);
        format!(
            "{} {}",
            shell_launcher(),
            encode_powershell_script(&payload)
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "{} '{}' {}  {}",
            shell_launcher(),
            script_path.display(),
            action,
            marker
        )
    }
}

/// Codex hook events and the state action each reports. SessionStart carries
/// the native session id; the rest report a state. `Stop` fires at the end of
/// every turn, so it maps to `idle` — it marks "thinking finished", not a pane
/// release. `SessionEnd` fires once when the session terminates, so it maps to
/// `released`, which clears the pane's agent identity.
const CODEX_HOOK_EVENTS: &[(&str, &str)] = &[
    ("SessionStart", "session"),
    ("UserPromptSubmit", "working"),
    ("PreToolUse", "working"),
    ("PostToolUse", "working"),
    ("PermissionRequest", "blocked"),
    ("SessionEnd", "released"),
    ("Stop", "idle"),
];

/// Claude Code hook events and the state action each reports. Same mapping as
/// Codex; Claude Code has no PermissionRequest event, so blocked is not wired
/// here.
const CLAUDE_HOOK_EVENTS: &[(&str, &str)] = &[
    ("SessionStart", "session"),
    ("UserPromptSubmit", "working"),
    ("PreToolUse", "working"),
    ("PostToolUse", "working"),
    ("SessionEnd", "released"),
    ("Stop", "idle"),
];

/// The shell that runs the hook script, by absolute path so the managed
/// command is self-contained and does not depend on the CLI's PATH.
fn shell_launcher() -> &'static str {
    #[cfg(windows)]
    {
        // Hook commands are executed by the agent's own hook runner. Codex
        // runs them via `cmd.exe /C`; Claude Code via Git Bash or PowerShell.
        // `-EncodedCommand` takes a base64 payload that contains no quotes or
        // spaces, so the command survives every one of those shells.
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -EncodedCommand"
    }
    #[cfg(not(windows))]
    {
        "/bin/sh"
    }
}

/// Marker prefix embedded in managed config entries. The full marker is
/// `# vintage:<agent>` so each integration is identifiable.
const CONFIG_MARKER: &str = "# vintage";

/// Reads the agent's config JSON (hooks.json for Codex, settings.json for
/// Claude), appends the managed hook entries, and writes it back. Codex gets
/// a SessionStart entry plus one per state-reporting event; Claude gets a
/// single SessionStart entry.
fn wire_config(agent: IntegrationAgent, home: &Path) -> Result<(), String> {
    if agent == IntegrationAgent::Opencode {
        return Ok(());
    }
    // Codex needs `[features] hooks = true` or it ignores hooks entirely.
    if agent == IntegrationAgent::Codex {
        ensure_codex_hooks_feature(home)?;
    }
    let path = config_path(agent, home)?;
    let mut doc = read_config_json(&path)?;

    // Create the hooks object when absent (fresh config or one without hooks).
    // A non-object hooks entry is refused so user data is never destroyed.
    let doc_obj = doc
        .as_object_mut()
        .ok_or_else(|| "The integration config is not an object.".to_string())?;
    let hooks = match doc_obj.get_mut("hooks") {
        Some(Value::Object(object)) => object,
        Some(_) => {
            return Err(format!(
                "{} has a non-object hooks entry; refusing to rewrite it.",
                path.display()
            ))
        }
        None => {
            doc_obj.insert("hooks".to_string(), Value::Object(Default::default()));
            doc_obj
                .get_mut("hooks")
                .and_then(Value::as_object_mut)
                .expect("hooks object inserted above")
        }
    };

    // The (event, action) pairs to wire for this agent.
    let events: &[(&str, &str)] = match agent {
        IntegrationAgent::Codex => CODEX_HOOK_EVENTS,
        IntegrationAgent::Claude => CLAUDE_HOOK_EVENTS,
        IntegrationAgent::Opencode => &[],
    };

    for (event, action) in events {
        let command = managed_hook_command(agent, home, action);
        let event_list = hooks
            .entry((*event).to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let array = event_list
            .as_array_mut()
            .ok_or_else(|| format!("The {event} config entry is not a list."))?;

        // Remove any prior managed entry, then append the current one (update
        // semantics) — this also deduplicates on repeated installs. The managed
        // command already ends with the `# vintage` marker, so matching on the
        // full command locates exactly the entry we own.
        array.retain(|entry| !entry_contains_marker(entry, &command));
        let entry = json_session_start_entry(agent, &command);
        array.push(entry);
    }

    write_config_json(&path, &doc)
}

/// Reads and merges the config file, preserving unknown fields. Missing files
/// start as an empty object.
fn read_config_json(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).map_err(|_| {
            format!(
                "{} is not valid JSON; refusing to rewrite it.",
                path.display()
            )
        }),
        Err(_) => Ok(Value::Object(Default::default())),
    }
}

fn write_config_json(path: &Path, doc: &Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "The integration config location is unavailable.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|_| "Could not prepare the integration config location.".to_string())?;
    let bytes = serde_json::to_vec_pretty(doc)
        .map_err(|_| "The integration config could not be serialized.".to_string())?;
    write_atomic(path, &bytes)
}

/// Codex ignores hooks unless `[features] hooks = true` is set in config.toml.
/// Ensures that key is present while preserving every other feature and key.
fn ensure_codex_hooks_feature(home: &Path) -> Result<(), String> {
    let root = config_root_with_home(IntegrationAgent::Codex, home)?;
    let path = root.join("config.toml");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => String::new(),
    };
    // Parse into a toml::Value, set features.hooks = true, write back. A
    // missing file (or one without a features table) parses to an empty doc.
    let doc: toml::Value = toml::from_str(&content).map_err(|_| {
        format!(
            "{} is not valid TOML; refusing to rewrite it.",
            path.display()
        )
    })?;
    let features_hooks = doc
        .get("features")
        .and_then(toml::Value::as_table)
        .and_then(|features| features.get("hooks"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    if features_hooks {
        return Ok(());
    }
    // Ensure doc is a table, then set features.hooks = true. The index
    // assignment needs existing tables; build them explicitly to avoid a panic
    // on a doc that is not yet shaped as { features: { hooks } }.
    let mut table = match doc {
        toml::Value::Table(table) => table,
        _ => toml::map::Map::new(),
    };
    let features = table
        .entry("features")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let features_table = match features {
        toml::Value::Table(table) => table,
        _ => {
            return Err(format!(
                "{} has a non-table features entry; refusing to rewrite it.",
                path.display()
            ))
        }
    };
    features_table.insert("hooks".to_string(), toml::Value::Boolean(true));
    let serialized = toml::to_string(&toml::Value::Table(table))
        .map_err(|_| "The Codex features config could not be serialized.".to_string())?;
    write_atomic(&path, serialized.as_bytes())
}

/// The managed SessionStart hook entry. Uses a `type: command` shell command
/// that runs the managed script with `session` (the same invocation the CLI
/// uses). The marker comment is embedded in the command string. Claude's
/// settings.json scopes SessionStart hooks with `matcher: "*"` so they run on
/// every session start; Codex entries need no matcher.
fn json_session_start_entry(agent: IntegrationAgent, command: &str) -> Value {
    let mut entry = serde_json::Map::new();
    if agent == IntegrationAgent::Claude {
        entry.insert("matcher".to_string(), Value::String("*".to_string()));
    }
    entry.insert(
        "hooks".to_string(),
        serde_json::json!([{ "type": "command", "command": command, "timeout": 10 }]),
    );
    Value::Object(entry)
}

/// True when a SessionStart entry's managed command matches the marker.
fn entry_contains_marker(entry: &Value, marker: &str) -> bool {
    let Some(hooks) = entry.get("hooks").and_then(Value::as_array) else {
        return false;
    };
    hooks.iter().any(|hook| {
        hook.get("command")
            .and_then(Value::as_str)
            .map(|cmd| command_contains_marker(cmd, marker))
            .unwrap_or(false)
    })
}

/// True when a managed command string matches the marker. Matches the exact
/// per-event marker (`# vintage:<agent>:<action>`), the bare per-agent marker
/// (`# vintage:<agent>`) from earlier versions, and — on Windows — the marker
/// inside the `-EncodedCommand` base64 payload, so legacy upgrades can clean
/// up entries created before the encoded form. The `marker` argument is a full
/// command, so extract the agent suffix rather than re-deriving a prefix from
/// it.
fn command_contains_marker(command: &str, marker: &str) -> bool {
    // Windows `-EncodedCommand`: the trailing token is a base64 payload whose
    // decoded text carries the marker.
    let encoded = command
        .split(' ')
        .next_back()
        .filter(|part| !part.is_empty());
    if let Some(decoded) = encoded.and_then(decode_powershell_script) {
        if decoded.contains(marker) {
            return true;
        }
    }
    // Unix and legacy `-File` forms carry the marker directly in the command.
    command.contains(marker)
        || (command.contains(CONFIG_MARKER)
            && command.contains(&format!(":{}", agent_marker_suffix(marker))))
}

/// The `<agent>` suffix of a managed marker, e.g. `codex` for a marker that
/// ends with `# vintage:codex` or `# vintage:codex:session`.
fn agent_marker_suffix(marker: &str) -> String {
    let prefix = format!("{}:", CONFIG_MARKER);
    let Some(rest) = marker
        .find(&prefix)
        .map(|idx| &marker[idx + prefix.len()..])
    else {
        return String::new();
    };
    rest.split(':').next().unwrap_or("").to_string()
}

/// The agent's config JSON path (hooks.json for Codex, settings.json for
/// Claude). OpenCode has no config wiring.
fn config_path(agent: IntegrationAgent, home: &Path) -> Result<PathBuf, String> {
    match agent {
        IntegrationAgent::Codex => {
            let root = config_root_with_home(agent, home)?;
            Ok(root.join("hooks.json"))
        }
        IntegrationAgent::Claude => {
            let root = config_root_with_home(agent, home)?;
            Ok(root.join("settings.json"))
        }
        IntegrationAgent::Opencode => Err("OpenCode has no config wiring.".to_string()),
    }
}

/// Removes every managed hook entry from the agent's config, leaving all user
/// hooks and settings intact.
fn unwire_config(agent: IntegrationAgent, home: &Path) -> Result<(), String> {
    if agent == IntegrationAgent::Opencode {
        return Ok(());
    }
    let path = config_path(agent, home)?;
    if !path.exists() {
        return Ok(());
    }
    let mut doc = read_config_json(&path)?;

    let events: &[(&str, &str)] = match agent {
        IntegrationAgent::Codex => CODEX_HOOK_EVENTS,
        IntegrationAgent::Claude => CLAUDE_HOOK_EVENTS,
        IntegrationAgent::Opencode => &[],
    };

    let Some(hooks) = doc.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let mut removed_any = false;
    for (event, action) in events {
        let command = managed_hook_command(agent, home, action);
        let Some(event_list) = hooks.get_mut(*event).and_then(Value::as_array_mut) else {
            continue;
        };
        let before = event_list.len();
        event_list.retain(|entry| !entry_contains_marker(entry, &command));
        if event_list.len() != before {
            removed_any = true;
        }
        if event_list.is_empty() {
            hooks.remove(*event);
        }
    }
    if !removed_any {
        // Nothing was managed here; leave the file untouched.
        return Ok(());
    }
    // Drop empty containers so an install-then-uninstall round trip restores
    // the file to its original shape.
    if hooks.is_empty() {
        if let Value::Object(object) = &mut doc {
            object.remove("hooks");
        }
    }

    write_config_json(&path, &doc)
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
    include_str!("../assets/integrations/codex/vintage-agent-state.sh")
}

#[cfg(windows)]
fn codex_script() -> &'static str {
    include_str!("../assets/integrations/codex/vintage-agent-state.ps1")
}

#[cfg(not(windows))]
fn claude_script() -> &'static str {
    include_str!("../assets/integrations/claude/vintage-agent-state.sh")
}

#[cfg(windows)]
fn claude_script() -> &'static str {
    include_str!("../assets/integrations/claude/vintage-agent-state.ps1")
}

fn opencode_script() -> &'static str {
    include_str!("../assets/integrations/opencode/vintage-agent-state.js")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    /// Creates an isolated temp dir and runs the closure with it as home.
    fn with_temp_home<T>(run: impl FnOnce(&Path) -> T) -> T {
        let temp = std::env::temp_dir().join(format!(
            "vintage-integration-test-{}-{}",
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

            // Status reflects both the script and the wired config entry.
            assert_eq!(
                status_with_home(IntegrationAgent::Codex, home)
                    .expect("status")
                    .state,
                IntegrationState::Installed
            );

            let removed =
                uninstall_with_home(IntegrationAgent::Codex, home).expect("uninstall succeeds");
            assert_eq!(removed.state, IntegrationState::NotInstalled);
            assert!(!path.exists());
        });
    }

    #[test]
    fn a_managed_script_without_wiring_reports_not_installed() {
        with_temp_home(|home| {
            // Place the managed script but no hooks.json: not wired.
            let path = script_path_with_home(IntegrationAgent::Codex, home).expect("path");
            fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            fs::write(&path, codex_script()).expect("script");

            let st = status_with_home(IntegrationAgent::Codex, home).expect("status");
            assert_eq!(st.state, IntegrationState::NotInstalled);
        });
    }

    #[test]
    fn install_wires_the_session_start_entry_and_uninstall_removes_it() {
        with_temp_home(|home| {
            let _ = install_with_home(IntegrationAgent::Codex, home).expect("install");
            // Codex config.toml gets [features] hooks = true so hooks run.
            let config_toml = config_root_with_home(IntegrationAgent::Codex, home)
                .expect("root")
                .join("config.toml");
            let toml_doc: toml::Value =
                toml::from_str(&fs::read_to_string(&config_toml).expect("config.toml"))
                    .expect("valid toml");
            assert_eq!(
                toml_doc["features"]["hooks"],
                toml::Value::Boolean(true),
                "codex hooks feature is enabled"
            );

            let config = config_root_with_home(IntegrationAgent::Codex, home)
                .expect("root")
                .join("hooks.json");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            let session_start = doc["hooks"]["SessionStart"]
                .as_array()
                .expect("SessionStart list");
            assert_eq!(session_start.len(), 1, "exactly one managed entry");
            let command = session_start[0]["hooks"][0]["command"]
                .as_str()
                .expect("command");
            assert!(
                command_contains_marker(
                    command,
                    &format!("{}:{}:{}", CONFIG_MARKER, "codex", "session")
                ),
                "managed entry carries the marker"
            );

            // Re-installing keeps a single entry (no duplicates).
            let _ = install_with_home(IntegrationAgent::Codex, home).expect("reinstall");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            let session_start = doc["hooks"]["SessionStart"]
                .as_array()
                .expect("SessionStart list");
            assert_eq!(session_start.len(), 1, "reinstall stays deduplicated");

            let _ = uninstall_with_home(IntegrationAgent::Codex, home).expect("uninstall");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            // Uninstall restores the config to its original empty shape.
            assert_eq!(doc, Value::Object(Default::default()));
        });
    }

    #[test]
    fn codex_install_wires_every_state_event_and_uninstall_removes_them() {
        with_temp_home(|home| {
            let _ = install_with_home(IntegrationAgent::Codex, home).expect("install");
            let config = config_root_with_home(IntegrationAgent::Codex, home)
                .expect("root")
                .join("hooks.json");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            let hooks = doc["hooks"].as_object().expect("hooks object");

            // Every Codex event has exactly one managed entry with the marker.
            for (event, action) in CODEX_HOOK_EVENTS {
                let list = hooks
                    .get(*event)
                    .and_then(Value::as_array)
                    .unwrap_or_else(|| panic!("missing {event}"));
                assert_eq!(list.len(), 1, "{event} should have one managed entry");
                let command = list[0]["hooks"][0]["command"].as_str().expect("command");
                let marker = format!("{}:{}:{}", CONFIG_MARKER, "codex", action);
                assert!(
                    command_contains_marker(command, &marker),
                    "{event} command should carry its marker: {command}"
                );
                assert!(
                    command.contains(action),
                    "{event} command should run the {action} action: {command}"
                );
            }

            let _ = uninstall_with_home(IntegrationAgent::Codex, home).expect("uninstall");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            // All managed events are removed; the file is back to empty.
            assert_eq!(doc, Value::Object(Default::default()));
        });
    }

    #[test]
    fn claude_install_wires_state_events_and_uninstall_removes_them() {
        with_temp_home(|home| {
            let _ = install_with_home(IntegrationAgent::Claude, home).expect("install");
            let config = config_root_with_home(IntegrationAgent::Claude, home)
                .expect("root")
                .join("settings.json");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            let hooks = doc["hooks"].as_object().expect("hooks object");

            // Every Claude event has exactly one managed entry with the marker.
            for (event, action) in CLAUDE_HOOK_EVENTS {
                let list = hooks
                    .get(*event)
                    .and_then(Value::as_array)
                    .unwrap_or_else(|| panic!("missing {event}"));
                assert_eq!(list.len(), 1, "{event} should have one managed entry");
                let command = list[0]["hooks"][0]["command"].as_str().expect("command");
                let marker = format!("{}:{}:{}", CONFIG_MARKER, "claude", action);
                assert!(
                    command_contains_marker(command, &marker),
                    "{event} command should carry its marker: {command}"
                );
            }

            let _ = uninstall_with_home(IntegrationAgent::Claude, home).expect("uninstall");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            assert_eq!(doc, Value::Object(Default::default()));
        });
    }

    #[test]
    fn unwire_removes_legacy_bare_marker_entries() {
        with_temp_home(|home| {
            let config = config_root_with_home(IntegrationAgent::Codex, home)
                .expect("root")
                .join("hooks.json");
            // A legacy SessionStart entry with the bare per-agent marker
            // (`# vintage:codex`) that predates per-event markers.
            let legacy = serde_json::json!({
                "hooks": {
                    "SessionStart": [{
                        "hooks": [{
                            "type": "command",
                            "command": "/bin/sh 'x' session  # vintage:codex",
                            "timeout": 10
                        }]
                    }]
                }
            });
            fs::create_dir_all(config.parent().expect("parent")).expect("dir");
            fs::write(&config, serde_json::to_vec_pretty(&legacy).expect("json")).expect("write");

            unwire_config(IntegrationAgent::Codex, home).expect("unwire");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            assert_eq!(doc, Value::Object(Default::default()));
        });
    }

    #[cfg(windows)]
    #[test]
    fn encoded_windows_command_carries_the_marker_in_its_payload() {
        // The Windows managed command base64-encodes the whole invocation. The
        // marker must still be found inside the encoded payload so install
        // dedup, uninstall, and status checks locate the managed entry.
        let command = hook_command(
            &Path::new(r"C:\Users\me\.codex\vintage-agent-state.ps1"),
            "working",
            "# vintage:codex:working",
        );
        assert!(
            command
                .starts_with("powershell.exe -NoProfile -ExecutionPolicy Bypass -EncodedCommand "),
            "Windows command uses -EncodedCommand: {command}"
        );
        assert!(
            command_contains_marker(&command, "# vintage:codex:working"),
            "marker must be found inside the encoded payload: {command}"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_command_carries_the_marker_directly() {
        let command = hook_command(
            &Path::new("/home/me/.codex/vintage-agent-state.sh"),
            "working",
            "# vintage:codex:working",
        );
        assert!(
            command_contains_marker(&command, "# vintage:codex:working"),
            "Unix command carries the marker directly: {command}"
        );
    }

    #[test]
    fn legacy_bare_agent_markers_are_still_detected() {
        // Entries from earlier versions carried only `# vintage:<agent>` with
        // no action suffix. A per-event marker lookup must still locate them.
        let legacy = "/bin/sh 'x' session  # vintage:codex";
        assert!(
            command_contains_marker(legacy, "# vintage:codex:session"),
            "legacy bare marker matches a session lookup"
        );
        assert!(
            command_contains_marker(legacy, "# vintage:codex:working"),
            "legacy bare marker matches a working lookup"
        );
        assert!(
            !command_contains_marker(legacy, "# vintage:claude:session"),
            "another agent's marker never matches"
        );
    }

    #[test]
    fn encoded_windows_command_unwires_on_uninstall() {
        with_temp_home(|home| {
            let script = script_path_with_home(IntegrationAgent::Codex, home).expect("path");
            fs::create_dir_all(script.parent().expect("parent")).expect("dir");
            fs::write(&script, codex_script()).expect("script");

            // Simulate a Windows-style encoded command wired into hooks.json.
            let command = hook_command(&script, "session", "# vintage:codex:session");
            let doc = serde_json::json!({
                "hooks": {
                    "SessionStart": [{
                        "hooks": [{
                            "type": "command",
                            "command": command,
                            "timeout": 10
                        }]
                    }]
                }
            });
            let config = config_root_with_home(IntegrationAgent::Codex, home)
                .expect("root")
                .join("hooks.json");
            fs::write(&config, serde_json::to_vec_pretty(&doc).expect("json")).expect("write");

            unwire_config(IntegrationAgent::Codex, home).expect("unwire");
            let after: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            assert_eq!(after, Value::Object(Default::default()));
        });
    }

    #[test]
    fn wiring_preserves_existing_user_hooks() {
        with_temp_home(|home| {
            let config = config_root_with_home(IntegrationAgent::Claude, home)
                .expect("root")
                .join("settings.json");
            fs::create_dir_all(config.parent().expect("parent")).expect("dir");
            // A user-owned SessionStart entry and an unrelated setting.
            let user_config = serde_json::json!({
                "hooks": {
                    "SessionStart": [
                        {
                            "matcher": "startup|resume",
                            "hooks": [
                                { "type": "command", "command": "echo user-hook" }
                            ]
                        }
                    ]
                },
                "model": "haiku"
            });
            fs::write(
                &config,
                serde_json::to_vec_pretty(&user_config).expect("json"),
            )
            .expect("write");

            let _ = install_with_home(IntegrationAgent::Claude, home).expect("install");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");

            // The user hook survives alongside the managed entry.
            let session_start = doc["hooks"]["SessionStart"]
                .as_array()
                .expect("SessionStart list");
            assert_eq!(session_start.len(), 2, "user hook + managed entry");
            assert_eq!(doc["model"], "haiku", "unrelated settings preserved");
            let managed = session_start
                .iter()
                .find(|entry| {
                    entry["hooks"][0]["command"].as_str().is_some_and(|c| {
                        command_contains_marker(
                            c,
                            &format!("{}:{}:{}", CONFIG_MARKER, "claude", "session"),
                        )
                    })
                })
                .expect("managed entry present");
            assert_eq!(
                managed["matcher"], "*",
                "claude matcher scopes all sessions"
            );

            let _ = uninstall_with_home(IntegrationAgent::Claude, home).expect("uninstall");
            let doc: Value = serde_json::from_str(&fs::read_to_string(&config).expect("config"))
                .expect("valid json");
            let session_start = doc["hooks"]["SessionStart"]
                .as_array()
                .expect("SessionStart list");
            assert_eq!(session_start.len(), 1, "user hook survives uninstall");
            assert_eq!(doc["model"], "haiku", "settings survive uninstall");
        });
    }

    #[test]
    fn invalid_existing_config_is_never_rewritten() {
        with_temp_home(|home| {
            let config = config_root_with_home(IntegrationAgent::Codex, home)
                .expect("root")
                .join("hooks.json");
            fs::create_dir_all(config.parent().expect("parent")).expect("dir");
            fs::write(&config, "not-json{").expect("write");

            assert!(install_with_home(IntegrationAgent::Codex, home).is_err());
            assert_eq!(
                fs::read_to_string(&config).expect("still user content"),
                "not-json{",
                "malformed config is left untouched"
            );
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
            assert!(path.ends_with(PathBuf::from("plugins/vintage-agent-state.js")));
            assert!(path.exists());
        });
    }
}
