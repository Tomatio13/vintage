//! Shell detection and launch contracts.
//!
//! Standard shells are referenced by stable ids and re-resolved on every
//! launch; only custom shells persist a normalized absolute path, which the
//! host validates before trusting. Detection logic is expressed over
//! injected filesystem and environment inputs so every Windows and Unix
//! branch can be unit tested on any OS. Command construction (POSIX
//! quoting, PowerShell EncodedCommand) is pure and always compiled.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

// Stable ids persisted in pane definitions.
pub(crate) const SHELL_ID_WINDOWS_DEFAULT: &str = "windows-default";
// Windows-only ids; referenced by cross-platform tests, unused on Unix builds.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const SHELL_ID_WINDOWS_PWSH: &str = "windows-pwsh";
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const SHELL_ID_WINDOWS_POWERSHELL: &str = "windows-powershell";
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const SHELL_ID_WINDOWS_GIT_BASH: &str = "windows-git-bash";
pub(crate) const SHELL_ID_UNIX_DEFAULT: &str = "unix-default";
pub(crate) const SHELL_ID_UNIX_BASH: &str = "unix-bash";
pub(crate) const SHELL_ID_UNIX_ZSH: &str = "unix-zsh";
pub(crate) const SHELL_ID_UNIX_PWSH: &str = "unix-pwsh";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ShellKind {
    #[cfg_attr(not(windows), allow(dead_code))]
    Powershell,
    Pwsh,
    #[cfg_attr(not(windows), allow(dead_code))]
    GitBash,
    Bash,
    Zsh,
    Posix,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ShellPlatform {
    Windows,
    Unix,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShellDescriptor {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) kind: ShellKind,
    pub(crate) executable: String,
    pub(crate) platform: ShellPlatform,
    pub(crate) available: bool,
    pub(crate) supports_agent_wrapper: bool,
}

/// A shell resolved for launch: descriptor plus base arguments and extra
/// environment entries. The caller still sets TERM/COLORTERM and the cwd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedShell {
    pub(crate) descriptor: ShellDescriptor,
    pub(crate) args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
}

/// Injected host inputs so detection stays testable without real shells.
pub(crate) struct ShellDetectionInput<'a> {
    /// True when the path exists and is a regular file.
    pub(crate) exists: &'a dyn Fn(&Path) -> bool,
    /// Resolves an executable name against PATH.
    pub(crate) on_path: &'a dyn Fn(&str) -> Option<PathBuf>,
    /// Reads an environment variable.
    pub(crate) env: &'a dyn Fn(&str) -> Option<String>,
    /// Login shell from the passwd database (Unix only).
    pub(crate) passwd_shell: Option<String>,
}

/// Detects the shells offered on the current platform.
pub(crate) fn detect_shells() -> Vec<ShellDescriptor> {
    let exists = |path: &Path| -> bool {
        std::fs::metadata(path)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
    };
    let on_path = |name: &str| -> Option<PathBuf> { which_on_path(name) };
    let env = |name: &str| -> Option<String> { std::env::var(name).ok() };
    let input = ShellDetectionInput {
        exists: &exists,
        on_path: &on_path,
        env: &env,
        passwd_shell: passwd_login_shell(),
    };
    #[cfg(windows)]
    {
        detect_windows_shells(&input)
    }
    #[cfg(not(windows))]
    {
        detect_unix_shells(&input)
    }
}

pub(crate) fn which_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path_var) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn passwd_login_shell() -> Option<String> {
    let user = std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("LOGNAME").ok())?;
    let content = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd_login_shell_from(&content, &user)
}

#[cfg(not(unix))]
fn passwd_login_shell() -> Option<String> {
    None
}

/// Pure passwd parser: returns the login shell field for `user`.
pub(crate) fn passwd_login_shell_from(content: &str, user: &str) -> Option<String> {
    for line in content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 7 && fields[0] == user {
            let shell = fields[6].trim();
            if !shell.is_empty() {
                return Some(shell.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Windows detection
// ---------------------------------------------------------------------------

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn detect_windows_shells(input: &ShellDetectionInput<'_>) -> Vec<ShellDescriptor> {
    let mut shells = Vec::new();

    // PowerShell 7: PATH first, then %ProgramFiles%\PowerShell\7\pwsh.exe.
    let pwsh_program_files = (input.env)("ProgramFiles").map(|program_files| {
        PathBuf::from(program_files)
            .join("PowerShell")
            .join("7")
            .join("pwsh.exe")
    });
    let pwsh_path = (input.on_path)("pwsh.exe")
        .or(pwsh_program_files)
        .filter(|path| (input.exists)(path));

    // Windows PowerShell 5.1: %SystemRoot% location first, then PATH.
    let system_root_powershell = (input.env)("SystemRoot").map(|system_root| {
        PathBuf::from(system_root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe")
    });
    let powershell_path = system_root_powershell
        .filter(|path| (input.exists)(path))
        .or_else(|| (input.on_path)("powershell.exe"))
        .filter(|path| (input.exists)(path));

    // Git Bash: PATH candidate (verified against its Git root), then known
    // Git for Windows install locations. WSL launchers and unrelated MSYS2
    // bash binaries fail the cmd\git.exe sibling check and are never picked.
    let git_bash_path = windows_git_bash_path(input);

    let default_is_pwsh = pwsh_path.is_some();
    let default_executable = pwsh_path
        .clone()
        .or_else(|| powershell_path.clone())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    shells.push(ShellDescriptor {
        id: SHELL_ID_WINDOWS_DEFAULT.to_string(),
        label: if default_is_pwsh {
            "PowerShell 7 (default)".to_string()
        } else {
            "Windows PowerShell (default)".to_string()
        },
        kind: if default_is_pwsh {
            ShellKind::Pwsh
        } else {
            ShellKind::Powershell
        },
        executable: default_executable,
        platform: ShellPlatform::Windows,
        available: default_is_pwsh || powershell_path.is_some(),
        supports_agent_wrapper: true,
    });
    shells.push(windows_descriptor(
        SHELL_ID_WINDOWS_PWSH,
        "PowerShell 7",
        ShellKind::Pwsh,
        pwsh_path,
    ));
    shells.push(windows_descriptor(
        SHELL_ID_WINDOWS_POWERSHELL,
        "Windows PowerShell 5.1",
        ShellKind::Powershell,
        powershell_path,
    ));
    shells.push(windows_descriptor(
        SHELL_ID_WINDOWS_GIT_BASH,
        "Git Bash",
        ShellKind::GitBash,
        git_bash_path,
    ));
    shells
}

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_descriptor(
    id: &str,
    label: &str,
    kind: ShellKind,
    path: Option<PathBuf>,
) -> ShellDescriptor {
    ShellDescriptor {
        id: id.to_string(),
        label: label.to_string(),
        kind,
        executable: path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        platform: ShellPlatform::Windows,
        available: path.is_some(),
        supports_agent_wrapper: true,
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_git_bash_path(input: &ShellDetectionInput<'_>) -> Option<PathBuf> {
    if let Some(path_candidate) = (input.on_path)("bash.exe") {
        if git_root_has_git_exe(&path_candidate, input.exists) {
            return Some(path_candidate);
        }
    }
    let install_roots: Vec<PathBuf> = [
        (input.env)("ProgramFiles").map(PathBuf::from),
        (input.env)("ProgramFiles(x86)").map(PathBuf::from),
        (input.env)("LOCALAPPDATA").map(|local| PathBuf::from(local).join("Programs")),
    ]
    .into_iter()
    .flatten()
    .collect();
    for root in install_roots {
        let candidate = root.join("Git").join("bin").join("bash.exe");
        if (input.exists)(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// A PATH bash.exe is Git Bash only when `<git root>\cmd\git.exe` exists
/// next to `<git root>\bin\bash.exe`. WSL's `System32\bash.exe` and stray
/// MSYS2 builds have no such sibling and are rejected.
#[cfg_attr(not(windows), allow(dead_code))]
fn git_root_has_git_exe(bash_path: &Path, exists: impl Fn(&Path) -> bool + Copy) -> bool {
    bash_path
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("cmd").join("git.exe"))
        .map(|git| exists(&git))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Unix detection
// ---------------------------------------------------------------------------

pub(crate) fn detect_unix_shells(input: &ShellDetectionInput<'_>) -> Vec<ShellDescriptor> {
    let mut shells = Vec::new();

    // Default shell: $SHELL when usable, then the passwd login shell, then
    // /bin/bash, and finally /bin/sh.
    let shell_env = (input.env)("SHELL").map(PathBuf::from);
    let default_candidates = [
        shell_env.filter(|path| (input.exists)(path)),
        input
            .passwd_shell
            .as_ref()
            .map(PathBuf::from)
            .filter(|path| (input.exists)(path)),
        Some(PathBuf::from("/bin/bash")).filter(|path| (input.exists)(path)),
        Some(PathBuf::from("/bin/sh")).filter(|path| (input.exists)(path)),
    ];
    let default_path = default_candidates.into_iter().flatten().next();

    let default_kind = default_path
        .as_ref()
        .map(|path| unix_kind_from_path(path))
        .unwrap_or(ShellKind::Posix);
    shells.push(ShellDescriptor {
        id: SHELL_ID_UNIX_DEFAULT.to_string(),
        label: "Default shell".to_string(),
        kind: default_kind,
        executable: default_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        platform: ShellPlatform::Unix,
        available: default_path.is_some(),
        supports_agent_wrapper: true,
    });

    let bash_path = (input.on_path)("bash")
        .or_else(|| Some(PathBuf::from("/bin/bash")))
        .filter(|path| (input.exists)(path));
    shells.push(unix_descriptor(
        SHELL_ID_UNIX_BASH,
        "Bash",
        ShellKind::Bash,
        bash_path,
    ));

    let zsh_path = (input.on_path)("zsh").filter(|path| (input.exists)(path));
    shells.push(unix_descriptor(
        SHELL_ID_UNIX_ZSH,
        "Zsh",
        ShellKind::Zsh,
        zsh_path,
    ));

    let pwsh_path = (input.on_path)("pwsh").filter(|path| (input.exists)(path));
    shells.push(unix_descriptor(
        SHELL_ID_UNIX_PWSH,
        "PowerShell 7",
        ShellKind::Pwsh,
        pwsh_path,
    ));
    shells
}

fn unix_descriptor(
    id: &str,
    label: &str,
    kind: ShellKind,
    path: Option<PathBuf>,
) -> ShellDescriptor {
    ShellDescriptor {
        id: id.to_string(),
        label: label.to_string(),
        kind,
        executable: path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        platform: ShellPlatform::Unix,
        available: path.is_some(),
        supports_agent_wrapper: true,
    }
}

fn unix_kind_from_path(path: &Path) -> ShellKind {
    match path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .as_deref()
    {
        Some("bash") => ShellKind::Bash,
        Some("zsh") => ShellKind::Zsh,
        Some("pwsh") => ShellKind::Pwsh,
        _ => ShellKind::Posix,
    }
}

// ---------------------------------------------------------------------------
// Resolution and executable validation
// ---------------------------------------------------------------------------

/// Resolves a persisted shell id to a launchable shell. Standard ids are
/// matched against a fresh detection pass; any other value must be a
/// normalized absolute path to a validated executable (custom shell).
pub(crate) fn resolve_shell(
    detected: &[ShellDescriptor],
    shell_id: &str,
) -> Result<ResolvedShell, String> {
    if let Some(descriptor) = detected.iter().find(|shell| shell.id == shell_id) {
        if !descriptor.available {
            return Err(format!(
                "The shell \"{}\" is not available on this computer.",
                descriptor.label
            ));
        }
        return Ok(resolved_from_descriptor(descriptor.clone()));
    }
    let custom = resolve_custom_shell(shell_id)?;
    Ok(resolved_from_descriptor(custom))
}

/// Validates a renderer-supplied executable path and wraps it as a custom
/// shell descriptor. The path is never trusted unchecked.
pub(crate) fn resolve_custom_shell(executable: &str) -> Result<ShellDescriptor, String> {
    let path = PathBuf::from(executable);
    if !path.is_absolute() {
        return Err("A custom shell must be an absolute path.".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "The custom shell executable is unavailable.".to_string())?;
    validate_shell_executable(&canonical)?;
    let label = canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Custom shell".to_string());
    Ok(ShellDescriptor {
        id: canonical.to_string_lossy().into_owned(),
        label,
        kind: ShellKind::Custom,
        executable: canonical.to_string_lossy().into_owned(),
        platform: current_platform(),
        available: true,
        supports_agent_wrapper: false,
    })
}

fn current_platform() -> ShellPlatform {
    #[cfg(windows)]
    {
        ShellPlatform::Windows
    }
    #[cfg(not(windows))]
    {
        ShellPlatform::Unix
    }
}

fn resolved_from_descriptor(descriptor: ShellDescriptor) -> ResolvedShell {
    let mut env = BTreeMap::new();
    let args: Vec<String> = match descriptor.kind {
        ShellKind::Powershell | ShellKind::Pwsh => {
            vec!["-NoLogo".to_string(), "-NoExit".to_string()]
        }
        ShellKind::GitBash => {
            // Keep the selected workspace as the starting directory instead
            // of jumping home, and give agents a capable terminal.
            env.insert("CHERE_INVOKING".to_string(), "1".to_string());
            env.insert("TERM".to_string(), "xterm-256color".to_string());
            env.insert("COLORTERM".to_string(), "truecolor".to_string());
            vec!["--login".to_string(), "-i".to_string()]
        }
        _ => Vec::new(),
    };
    ResolvedShell {
        descriptor,
        args,
        env,
    }
}

/// Launch contract for an agent inside a shell. `descriptor` is the shell
/// whose interactive session owns the PTY; `args` launch the agent and then
/// return to the same shell (`-NoExit` / `exec bash`). `env` carries only
/// non-secret values; hook IPC tokens are injected separately by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentWrapperCommand {
    pub(crate) descriptor: ShellDescriptor,
    pub(crate) args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
}

/// Builds the shell invocation that runs an agent argv and returns to the
/// interactive shell. PowerShell uses `-EncodedCommand`; every other shell
/// uses POSIX single-quote escaping.
pub(crate) fn agent_wrapper_command(
    descriptor: ShellDescriptor,
    agent_argv: &[String],
) -> AgentWrapperCommand {
    let mut env = BTreeMap::new();
    let args = match descriptor.kind {
        ShellKind::Powershell | ShellKind::Pwsh => {
            let script = powershell_agent_script(agent_argv);
            vec![
                "-NoLogo".to_string(),
                "-NoExit".to_string(),
                "-EncodedCommand".to_string(),
                encode_powershell_script(&script),
            ]
        }
        ShellKind::GitBash | ShellKind::Bash | ShellKind::Zsh | ShellKind::Posix => {
            env.insert("TERM".to_string(), "xterm-256color".to_string());
            env.insert("COLORTERM".to_string(), "truecolor".to_string());
            bash_agent_arguments(agent_argv)
        }
        ShellKind::Custom => {
            // The custom executable is treated as a shell that already knows
            // how to resume; run the agent argv directly and let the caller
            // decide whether the shell stays open.
            let joined = agent_argv
                .iter()
                .map(|argument| posix_quote(argument))
                .collect::<Vec<_>>()
                .join(" ");
            vec![
                "-c".to_string(),
                format!("{joined}; exec {}", descriptor.executable),
            ]
        }
    };
    AgentWrapperCommand {
        descriptor,
        args,
        env,
    }
}

/// Host-side executable validation: a regular file, executable on Unix, and
/// on Windows carrying an extension PATHEXT would accept.
pub(crate) fn validate_shell_executable(path: &Path) -> Result<(), String> {
    let metadata =
        std::fs::metadata(path).map_err(|_| "The shell executable is unavailable.".to_string())?;
    if !metadata.is_file() {
        return Err("The shell executable is not a file.".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("The shell executable is not runnable.".to_string());
        }
    }
    #[cfg(windows)]
    {
        let accepted =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        let extension = path
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default()
            .to_uppercase();
        let allowed = accepted
            .split(';')
            .any(|candidate| candidate.trim().to_uppercase() == extension);
        if !allowed {
            return Err("The shell executable type is not allowed.".to_string());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Command construction (pure; exercised by tests on every OS)
// ---------------------------------------------------------------------------

/// POSIX single-quote escaping safe for spaces, quotes, trailing
/// backslashes, and arbitrary Unicode.
pub(crate) fn posix_quote(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "'\\''"))
}

/// Arguments for an interactive login bash that runs the agent argv and then
/// returns to a fresh bash in the same pane.
pub(crate) fn bash_agent_arguments(agent_argv: &[String]) -> Vec<String> {
    let joined = agent_argv
        .iter()
        .map(|argument| posix_quote(argument))
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        "--login".to_string(),
        "-i".to_string(),
        "-c".to_string(),
        format!("{joined}; exec bash --login -i"),
    ]
}

/// PowerShell single-quote literal: backticks are not special inside single
/// quotes, so doubling the quote is the only escape needed.
pub(crate) fn powershell_quote(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "''"))
}

/// Script text for `-EncodedCommand` that runs the agent argv. `-NoExit` on
/// the outer invocation keeps the shell open after the agent exits.
pub(crate) fn powershell_agent_script(agent_argv: &[String]) -> String {
    let call = agent_argv
        .iter()
        .map(|argument| powershell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ");
    format!("& {call}")
}

/// UTF-16LE Base64 encoding required by PowerShell `-EncodedCommand`.
pub(crate) fn encode_powershell_script(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    BASE64_STANDARD.encode(bytes)
}

/// Decodes an `-EncodedCommand` payload back to script text (test helper and
/// diagnostics only; never used to trust external input).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn decode_powershell_script(encoded: &str) -> Option<String> {
    let bytes = BASE64_STANDARD.decode(encoded).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::{
        bash_agent_arguments, decode_powershell_script, detect_unix_shells, detect_windows_shells,
        encode_powershell_script, git_root_has_git_exe, passwd_login_shell_from, posix_quote,
        powershell_agent_script, powershell_quote, resolve_custom_shell, resolve_shell,
        validate_shell_executable, ShellDescriptor, ShellDetectionInput, ShellKind,
        SHELL_ID_UNIX_BASH, SHELL_ID_UNIX_DEFAULT, SHELL_ID_WINDOWS_DEFAULT,
        SHELL_ID_WINDOWS_GIT_BASH, SHELL_ID_WINDOWS_POWERSHELL, SHELL_ID_WINDOWS_PWSH,
    };
    use base64::Engine as _;
    use std::{
        collections::{HashMap, HashSet},
        path::{Path, PathBuf},
    };

    struct FakeHost {
        files: HashSet<PathBuf>,
        path_bins: HashMap<String, PathBuf>,
        env: HashMap<String, String>,
        passwd_shell: Option<String>,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                files: HashSet::new(),
                path_bins: HashMap::new(),
                env: HashMap::new(),
                passwd_shell: None,
            }
        }

        fn file(mut self, path: &str) -> Self {
            self.files.insert(PathBuf::from(path));
            self
        }

        fn bin(mut self, name: &str, path: &str) -> Self {
            let path = PathBuf::from(path);
            self.files.insert(path.clone());
            self.path_bins.insert(name.to_string(), path);
            self
        }

        fn env(mut self, name: &str, value: &str) -> Self {
            self.env.insert(name.to_string(), value.to_string());
            self
        }

        /// Builds the borrowed closures for one detection call.
        fn with_input<T>(&self, run: impl FnOnce(&ShellDetectionInput<'_>) -> T) -> T {
            let exists = |path: &Path| self.files.contains(path);
            let on_path = |name: &str| self.path_bins.get(name).cloned();
            let env = |name: &str| self.env.get(name).cloned();
            let input = ShellDetectionInput {
                exists: &exists,
                on_path: &on_path,
                env: &env,
                passwd_shell: self.passwd_shell.clone(),
            };
            run(&input)
        }
    }

    fn find<'a>(shells: &'a [ShellDescriptor], id: &str) -> &'a ShellDescriptor {
        shells
            .iter()
            .find(|shell| shell.id == id)
            .unwrap_or_else(|| panic!("missing shell {id}"))
    }

    #[test]
    fn windows_prefers_pwsh_as_the_default_shell() {
        let host = FakeHost::new()
            .bin("pwsh.exe", "C:/Tools/pwsh.exe")
            .env("SystemRoot", "C:/Windows")
            .file("C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe");
        let shells = host.with_input(detect_windows_shells);

        let default = find(&shells, SHELL_ID_WINDOWS_DEFAULT);
        assert_eq!(default.kind, ShellKind::Pwsh);
        assert_eq!(default.executable, "C:/Tools/pwsh.exe");
        assert!(default.available);

        let legacy = find(&shells, SHELL_ID_WINDOWS_POWERSHELL);
        assert_eq!(
            legacy.executable,
            "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe"
        );
        assert!(legacy.available);
    }

    #[test]
    fn windows_falls_back_to_powershell_5_1_and_program_files_pwsh() {
        let host = FakeHost::new()
            .env("SystemRoot", "C:/Windows")
            .env("ProgramFiles", "C:/Program Files")
            .file("C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe");
        let shells = host.with_input(detect_windows_shells);
        let default = find(&shells, SHELL_ID_WINDOWS_DEFAULT);
        assert_eq!(default.kind, ShellKind::Powershell);
        assert!(default.available);
        assert!(!find(&shells, SHELL_ID_WINDOWS_PWSH).available);

        let host = FakeHost::new()
            .env("ProgramFiles", "C:/Program Files")
            .file("C:/Program Files/PowerShell/7/pwsh.exe");
        let shells = host.with_input(detect_windows_shells);
        let default = find(&shells, SHELL_ID_WINDOWS_DEFAULT);
        assert_eq!(default.kind, ShellKind::Pwsh);
        assert_eq!(default.executable, "C:/Program Files/PowerShell/7/pwsh.exe");
    }

    #[test]
    fn windows_accepts_git_bash_only_with_a_git_root_sibling() {
        // PATH bash with cmd\git.exe sibling -> accepted.
        let host = FakeHost::new()
            .bin("bash.exe", "C:/Program Files/Git/bin/bash.exe")
            .file("C:/Program Files/Git/cmd/git.exe");
        let shells = host.with_input(detect_windows_shells);
        let git_bash = find(&shells, SHELL_ID_WINDOWS_GIT_BASH);
        assert!(git_bash.available);
        assert_eq!(git_bash.executable, "C:/Program Files/Git/bin/bash.exe");

        // WSL launcher: System32\bash.exe with no Git root -> rejected from
        // PATH, then found at a real Git install location instead.
        let host = FakeHost::new()
            .bin("bash.exe", "C:/Windows/System32/bash.exe")
            .env("ProgramFiles", "C:/Program Files")
            .file("C:/Program Files/Git/bin/bash.exe");
        let shells = host.with_input(detect_windows_shells);
        let git_bash = find(&shells, SHELL_ID_WINDOWS_GIT_BASH);
        assert_eq!(git_bash.executable, "C:/Program Files/Git/bin/bash.exe");

        // No Git anywhere -> unavailable.
        let host = FakeHost::new().bin("bash.exe", "C:/Windows/System32/bash.exe");
        let shells = host.with_input(detect_windows_shells);
        assert!(!find(&shells, SHELL_ID_WINDOWS_GIT_BASH).available);
    }

    #[test]
    fn git_root_sibling_check_rejects_wsl_and_accepts_git_for_windows() {
        let exists = |path: &Path| path == Path::new("C:/Program Files/Git/cmd/git.exe");
        assert!(git_root_has_git_exe(
            Path::new("C:/Program Files/Git/bin/bash.exe"),
            exists
        ));
        assert!(!git_root_has_git_exe(
            Path::new("C:/Windows/System32/bash.exe"),
            exists
        ));
    }

    #[test]
    fn git_bash_known_install_locations_are_tried_in_order() {
        let host = FakeHost::new()
            .env("ProgramFiles(x86)", "C:/Program Files (x86)")
            .env("LOCALAPPDATA", "C:/Users/me/AppData/Local")
            .file("C:/Users/me/AppData/Local/Programs/Git/bin/bash.exe");
        let shells = host.with_input(detect_windows_shells);
        assert_eq!(
            find(&shells, SHELL_ID_WINDOWS_GIT_BASH).executable,
            "C:/Users/me/AppData/Local/Programs/Git/bin/bash.exe"
        );
    }

    #[test]
    fn unix_default_prefers_shell_env_then_passwd_then_bin_bash_then_sh() {
        let host = FakeHost::new()
            .env("SHELL", "/usr/bin/zsh")
            .file("/usr/bin/zsh")
            .file("/bin/bash")
            .file("/bin/sh");
        let shells = host.with_input(detect_unix_shells);
        let default = find(&shells, SHELL_ID_UNIX_DEFAULT);
        assert_eq!(default.executable, "/usr/bin/zsh");
        assert_eq!(default.kind, ShellKind::Zsh);

        let mut host = FakeHost::new().file("/bin/bash").file("/bin/sh");
        host.passwd_shell = Some("/usr/bin/fish".to_string());
        host.files.insert(PathBuf::from("/usr/bin/fish"));
        let shells = host.with_input(detect_unix_shells);
        assert_eq!(
            find(&shells, SHELL_ID_UNIX_DEFAULT).executable,
            "/usr/bin/fish"
        );

        let host = FakeHost::new().file("/bin/bash").file("/bin/sh");
        let shells = host.with_input(detect_unix_shells);
        let default = find(&shells, SHELL_ID_UNIX_DEFAULT);
        assert_eq!(default.executable, "/bin/bash");
        assert_eq!(default.kind, ShellKind::Bash);

        let host = FakeHost::new().file("/bin/sh");
        let shells = host.with_input(detect_unix_shells);
        assert_eq!(find(&shells, SHELL_ID_UNIX_DEFAULT).executable, "/bin/sh");
    }

    #[test]
    fn unix_lists_bash_zsh_and_pwsh_by_availability() {
        let host = FakeHost::new()
            .env("SHELL", "/bin/bash")
            .bin("bash", "/bin/bash")
            .bin("zsh", "/usr/bin/zsh")
            .bin("pwsh", "/opt/microsoft/pwsh");
        let shells = host.with_input(detect_unix_shells);
        assert!(find(&shells, SHELL_ID_UNIX_BASH).available);
        assert!(find(&shells, "unix-zsh").available);
        assert!(find(&shells, "unix-pwsh").available);
        assert_eq!(find(&shells, "unix-pwsh").kind, ShellKind::Pwsh);
    }

    #[test]
    fn passwd_parser_reads_the_shell_field() {
        let content = "root:x:0:0:root:/root:/bin/bash\nme:x:1000:1000:Me:/home/me:/usr/bin/zsh\n";
        assert_eq!(
            passwd_login_shell_from(content, "me").as_deref(),
            Some("/usr/bin/zsh")
        );
        assert_eq!(
            passwd_login_shell_from(content, "root").as_deref(),
            Some("/bin/bash")
        );
        assert_eq!(passwd_login_shell_from(content, "ghost"), None);
    }

    #[test]
    fn posix_quote_survives_spaces_quotes_unicode_and_trailing_backslashes() {
        assert_eq!(posix_quote("plain"), "'plain'");
        assert_eq!(posix_quote("with space"), "'with space'");
        assert_eq!(posix_quote("it's"), "'it'\\''s'");
        assert_eq!(posix_quote("日本語"), "'日本語'");
        assert_eq!(posix_quote("trail\\"), "'trail\\'");
    }

    #[cfg(unix)]
    #[test]
    fn posix_quote_round_trips_through_a_real_bash() {
        use std::process::Command;
        for argument in [
            "plain",
            "with space",
            "it's quoted",
            "日本語の引数",
            "trailing\\",
            "$HOME `backtick` \"double\"",
        ] {
            let script = format!("printf %s {}", posix_quote(argument));
            let output = Command::new("bash")
                .arg("-c")
                .arg(&script)
                .output()
                .expect("bash should run");
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                argument,
                "argument must survive shell parsing"
            );
        }
    }

    #[test]
    fn bash_agent_arguments_keep_the_shell_open_after_the_agent() {
        let argv = vec![
            "codex".to_string(),
            "run with space".to_string(),
            "it's".to_string(),
        ];
        let args = bash_agent_arguments(&argv);
        assert_eq!(args[0], "--login");
        assert_eq!(args[1], "-i");
        assert_eq!(args[2], "-c");
        assert_eq!(
            args[3],
            "'codex' 'run with space' 'it'\\''s'; exec bash --login -i"
        );
    }

    #[test]
    fn powershell_quote_doubles_single_quotes_only() {
        assert_eq!(powershell_quote("plain"), "'plain'");
        assert_eq!(powershell_quote("it's"), "'it''s'");
        assert_eq!(
            powershell_quote("C:\\Program Files\\agent.exe"),
            "'C:\\Program Files\\agent.exe'"
        );
    }

    #[test]
    fn powershell_encoded_command_round_trips_utf16le_base64() {
        let argv = vec![
            "C:\\Program Files\\agent.exe".to_string(),
            "arg with space".to_string(),
            "日本語".to_string(),
            "it's".to_string(),
        ];
        let script = powershell_agent_script(&argv);
        assert_eq!(
            script,
            "& 'C:\\Program Files\\agent.exe' 'arg with space' '日本語' 'it''s'"
        );
        let encoded = encode_powershell_script(&script);
        assert_eq!(
            decode_powershell_script(&encoded).as_deref(),
            Some(script.as_str())
        );
        // UTF-16LE: first character '&' is 0x26 0x00 in the decoded bytes.
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .expect("valid base64");
        assert_eq!(&bytes[..2], &[0x26, 0x00]);
    }

    #[test]
    fn resolve_shell_maps_standard_ids_and_custom_paths() {
        let host = FakeHost::new().bin("bash", "/bin/bash");
        let shells = host.with_input(detect_unix_shells);
        let resolved = resolve_shell(&shells, SHELL_ID_UNIX_BASH).expect("bash resolves");
        assert_eq!(resolved.descriptor.executable, "/bin/bash");
        assert!(resolved.args.is_empty());

        assert!(
            resolve_shell(&shells, "unix-zsh").is_err(),
            "absent shell errors"
        );
    }

    #[test]
    fn git_bash_resolution_carries_chere_invoking_and_login_arguments() {
        let host = FakeHost::new()
            .env("ProgramFiles", "C:/Program Files")
            .file("C:/Program Files/Git/bin/bash.exe");
        let shells = host.with_input(detect_windows_shells);
        let resolved =
            resolve_shell(&shells, SHELL_ID_WINDOWS_GIT_BASH).expect("git bash resolves");
        assert_eq!(resolved.args, vec!["--login", "-i"]);
        assert_eq!(
            resolved.env.get("CHERE_INVOKING").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            resolved.env.get("TERM").map(String::as_str),
            Some("xterm-256color")
        );
    }

    #[cfg(unix)]
    #[test]
    fn custom_shell_validation_requires_absolute_executable_files() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let directory = std::env::temp_dir().join(format!(
            "vintage-shells-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&directory).expect("temp dir");
        let executable = directory.join("my-shell.sh");
        fs::write(&executable, "#!/bin/sh\n").expect("write");

        assert!(resolve_custom_shell("relative/path").is_err());
        assert!(
            resolve_custom_shell(executable.to_str().unwrap()).is_err(),
            "not executable yet"
        );

        let mut permissions = fs::metadata(&executable).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("chmod");

        let descriptor = resolve_custom_shell(executable.to_str().unwrap()).expect("valid shell");
        assert_eq!(descriptor.kind, ShellKind::Custom);
        assert!(!descriptor.supports_agent_wrapper);
        assert!(validate_shell_executable(&executable).is_ok());
        assert!(
            validate_shell_executable(&directory).is_err(),
            "directories are not shells"
        );

        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn descriptors_serialize_with_wire_casing() {
        let host = FakeHost::new()
            .env("ProgramFiles", "C:/Program Files")
            .file("C:/Program Files/Git/bin/bash.exe");
        let shells = host.with_input(detect_windows_shells);
        let value =
            serde_json::to_value(find(&shells, SHELL_ID_WINDOWS_GIT_BASH)).expect("serializes");
        assert_eq!(value["kind"], "git-bash");
        assert_eq!(value["platform"], "windows");
        assert_eq!(value["supportsAgentWrapper"], true);
        assert_eq!(value["id"], "windows-git-bash");
    }
}
