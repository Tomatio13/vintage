use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    sync::{mpsc, Arc, Mutex, MutexGuard},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};

use super::{
    shells::{self, ShellDescriptor},
    workspace_registry_host_path, workspaces,
};

const MIN_TERMINAL_DIMENSION: u16 = 2;
const MAX_TERMINAL_DIMENSION: u16 = 500;
const MAX_TERMINAL_WRITE_BYTES: usize = 64 * 1024;
const TERMINAL_OUTPUT_BATCH_BYTES: usize = 32 * 1024;
const TERMINAL_OUTPUT_BATCH_DELAY: Duration = Duration::from_millis(16);

pub(crate) struct TerminalSession {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    // Read by Phase 7 hook IPC to reject stale-generation reports.
    pub(crate) generation: u64,
    pub(crate) pane_id: String,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if let Some(killer) = self.killer.as_mut() {
            let _ = killer.kill();
        }
    }
}

#[derive(Default)]
pub(crate) struct TerminalRuntime {
    pub(crate) sessions: Mutex<HashMap<String, TerminalSession>>,
    workspace_roots: Mutex<HashMap<String, PathBuf>>,
}

impl TerminalRuntime {
    pub(crate) fn shutdown(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.clear();
        }
    }

    /// Removes a cached root when its registry entry is unregistered.
    pub(crate) fn forget_workspace(&self, workspace_id: &str) {
        if let Ok(mut roots) = self.workspace_roots.lock() {
            roots.remove(workspace_id);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalOutputEvent {
    terminal_id: String,
    generation: u64,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalExitEvent {
    terminal_id: String,
    generation: u64,
    exit_code: Option<u32>,
    signal: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AgentActivity {
    Unknown,
    Idle,
    Working,
    Blocked,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ActivitySource {
    Screen,
    #[serde(rename = "opencode-plugin")]
    OpencodePlugin,
    Runtime,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentActivityEvent {
    pub(crate) pane_id: String,
    pub(crate) generation: u64,
    pub(crate) activity: AgentActivity,
    pub(crate) source: ActivitySource,
    /// The CLI preset a hook/plugin report belongs to, or None for screen.
    pub(crate) agent: Option<String>,
    pub(crate) session_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalInfo {
    terminal_id: String,
    pane_id: String,
    generation: u64,
    working_directory: String,
    shell: ShellDescriptor,
    process_id: u32,
}

fn lock_sessions(
    runtime: &TerminalRuntime,
) -> Result<MutexGuard<'_, HashMap<String, TerminalSession>>, String> {
    runtime
        .sessions
        .lock()
        .map_err(|_| "The terminal service is unavailable.".to_string())
}

fn validate_terminal_id(terminal_id: &str) -> Result<(), String> {
    let valid = !terminal_id.is_empty()
        && terminal_id.len() <= 128
        && terminal_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err("The terminal identifier is invalid.".to_string())
    }
}

fn terminal_size(cols: u16, rows: u16) -> Result<PtySize, String> {
    let valid = (MIN_TERMINAL_DIMENSION..=MAX_TERMINAL_DIMENSION).contains(&cols)
        && (MIN_TERMINAL_DIMENSION..=MAX_TERMINAL_DIMENSION).contains(&rows);
    if !valid {
        return Err("The terminal dimensions are invalid.".to_string());
    }
    Ok(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}

/// Resolves a workspace id to its registered root directory. The renderer
/// passes only the id; the trusted path comes from the host registry.
fn resolve_workspace_directory(
    app: &AppHandle,
    runtime: &TerminalRuntime,
    workspace_id: &str,
) -> Result<PathBuf, String> {
    if let Ok(roots) = runtime.workspace_roots.lock() {
        if let Some(root) = roots.get(workspace_id) {
            if root.is_dir() {
                return Ok(root.clone());
            }
        }
    }
    let registry_path = workspace_registry_host_path(app).map_err(|error| error.message)?;
    let load = workspaces::load_registry_blocking(&registry_path).map_err(|error| error.message)?;
    let record = load
        .roots
        .iter()
        .find(|root| root.id == workspace_id)
        .ok_or_else(|| "That workspace is not registered.".to_string())?;
    let canonical = PathBuf::from(&record.path)
        .canonicalize()
        .map_err(|_| "The terminal working directory is unavailable.".to_string())?;
    if !canonical.is_dir() {
        return Err("The terminal working directory is not a folder.".to_string());
    }
    if let Ok(mut roots) = runtime.workspace_roots.lock() {
        roots.insert(workspace_id.to_string(), canonical.clone());
    }
    Ok(canonical)
}

/// What to spawn for a terminal. Workspace launches use the resolved shell
/// contract; agent and custom launches add the wrapper arguments.
struct LaunchPlan {
    program: PathBuf,
    arguments: Vec<String>,
    environment: Vec<(String, String)>,
    descriptor: ShellDescriptor,
}

/// A detectable agent preset: its CLI name plus the executable found on PATH.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentPresetInfo {
    pub(crate) preset: workspaces::AgentPreset,
    pub(crate) cli_name: String,
    pub(crate) executable: Option<String>,
    pub(crate) detected: bool,
    #[serde(skip_serializing)]
    pub(crate) resume_flag: Option<String>,
}

/// Preset launch resolutions. Grok has no native resume; the others map to
/// their documented resume flags (Phase 0 verified all four).
pub(crate) fn agent_preset_info() -> Vec<AgentPresetInfo> {
    let definitions = [
        (workspaces::AgentPreset::Grok, "grok", None),
        (workspaces::AgentPreset::Codex, "codex", Some("resume")),
        (workspaces::AgentPreset::Claude, "claude", Some("--resume")),
        (
            workspaces::AgentPreset::Opencode,
            "opencode",
            Some("--session"),
        ),
    ];
    definitions
        .into_iter()
        .map(|(preset, cli_name, resume_flag)| {
            let executable = shells::which_on_path(cli_name);
            AgentPresetInfo {
                preset,
                cli_name: cli_name.to_string(),
                executable: executable
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                detected: executable.is_some(),
                resume_flag: resume_flag.map(str::to_string),
            }
        })
        .collect()
}

fn plan_launch(
    app: &AppHandle,
    runtime: &TerminalRuntime,
    launch: &workspaces::PaneLaunchSpec,
    workspace_id: Option<String>,
) -> Result<(LaunchPlan, PathBuf), String> {
    match launch {
        workspaces::PaneLaunchSpec::Shell { shell_id } => {
            let workspace_id = workspace_id
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "A workspace is required to start a terminal.".to_string())?;
            let directory = resolve_workspace_directory(app, runtime, &workspace_id)?;
            let detected = shells::detect_shells();
            let resolved = shells::resolve_shell(&detected, &shell_id)?;
            Ok((
                LaunchPlan {
                    program: PathBuf::from(&resolved.descriptor.executable),
                    arguments: resolved.args,
                    environment: resolved.env.into_iter().collect(),
                    descriptor: resolved.descriptor,
                },
                directory,
            ))
        }
        workspaces::PaneLaunchSpec::Agent {
            preset,
            shell_id,
            args,
            resume_session_id,
        } => {
            let workspace_id = workspace_id
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "A workspace is required to start an agent.".to_string())?;
            let directory = resolve_workspace_directory(app, runtime, &workspace_id)?;
            let detected = shells::detect_shells();
            let resolved = shells::resolve_shell(&detected, &shell_id)?;
            let preset_info = agent_preset_info()
                .into_iter()
                .find(|info| info.preset == *preset)
                .ok_or_else(|| "That agent preset is unavailable.".to_string())?;
            let executable = preset_info
                .executable
                .ok_or_else(|| {
                    format!(
                        "The \"{}\" command was not found on PATH. You can still start a plain shell in this pane.",
                        preset_info.cli_name
                    )
                })?;

            // Compose the agent argv: executable + resume/args.
            let mut agent_argv = vec![executable.clone()];
            if let Some(resume) = &resume_session_id {
                if let Some(flag) = &preset_info.resume_flag {
                    agent_argv.push(flag.clone());
                    agent_argv.push(resume.clone());
                }
            }
            agent_argv.extend(args.iter().cloned());

            let wrapper = shells::agent_wrapper_command(resolved.descriptor.clone(), &agent_argv);
            Ok((
                LaunchPlan {
                    program: PathBuf::from(&resolved.descriptor.executable),
                    arguments: wrapper.args,
                    environment: wrapper.env.into_iter().collect(),
                    descriptor: wrapper.descriptor,
                },
                directory,
            ))
        }
        workspaces::PaneLaunchSpec::Custom { program, args } => {
            let workspace_id = workspace_id
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "A workspace is required to start a program.".to_string())?;
            let directory = resolve_workspace_directory(app, runtime, &workspace_id)?;
            let program_path = PathBuf::from(&program);
            if !program_path.is_absolute() {
                return Err("A custom program must be an absolute path.".to_string());
            }
            let canonical = program_path
                .canonicalize()
                .map_err(|_| "The custom program executable is unavailable.".to_string())?;
            shells::validate_shell_executable(&canonical)?;
            let executable = canonical.to_string_lossy().into_owned();
            let label = canonical
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Custom program".to_string());
            Ok((
                LaunchPlan {
                    program: canonical,
                    arguments: args.clone(),
                    environment: Vec::new(),
                    descriptor: ShellDescriptor {
                        id: executable.clone(),
                        label,
                        kind: shells::ShellKind::Custom,
                        executable,
                        platform: if cfg!(windows) {
                            shells::ShellPlatform::Windows
                        } else {
                            shells::ShellPlatform::Unix
                        },
                        available: true,
                        supports_agent_wrapper: false,
                    },
                },
                directory,
            ))
        }
    }
}

#[tauri::command]
pub(crate) fn terminal_start(
    app: AppHandle,
    state: State<'_, TerminalRuntime>,
    hook_runtime: State<'_, crate::hook_ipc::HookIpcRuntime>,
    terminal_id: String,
    pane_id: String,
    generation: u64,
    workspace_id: String,
    launch: workspaces::PaneLaunchSpec,
    cols: u16,
    rows: u16,
) -> Result<TerminalInfo, String> {
    validate_terminal_id(&terminal_id)?;
    validate_terminal_id(&pane_id)?;
    let size = terminal_size(cols, rows)?;
    let (plan, working_directory) = plan_launch(&app, &state, &launch, Some(workspace_id))?;

    if lock_sessions(&state)?.contains_key(&terminal_id) {
        return Err("That terminal is already running.".to_string());
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(size)
        .map_err(|_| "Failed to create a terminal.".to_string())?;
    let mut command = CommandBuilder::new(&plan.program);
    command.args(&plan.arguments);
    command.cwd(&working_directory);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    for (key, value) in &plan.environment {
        command.env(key, value);
    }
    // Hook IPC connection info goes to the PTY child's environment only; the
    // renderer never sees it after the token was handed to the host. The
    // agent preset name lets the hook and the receiver match a report to the
    // CLI actually driving this pane.
    let agent_name = match &launch {
        workspaces::PaneLaunchSpec::Agent { preset, .. } => preset.as_str(),
        _ => "",
    };
    for (key, value) in hook_runtime.child_env(&pane_id, generation, agent_name) {
        command.env(key, value);
    }

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|_| "Failed to read terminal output.".to_string())?;
    let writer = Arc::new(Mutex::new(
        pair.master
            .take_writer()
            .map_err(|_| "Failed to prepare terminal input.".to_string())?,
    ));
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|_| format!("Failed to start the shell \"{}\".", plan.descriptor.label))?;
    let process_id = child.process_id().unwrap_or(0);
    let killer = child.clone_killer();
    drop(pair.slave);

    let session = TerminalSession {
        writer,
        master: pair.master,
        killer: Some(killer),
        generation,
        pane_id: pane_id.clone(),
    };
    let mut sessions = lock_sessions(&state)?;
    if sessions.contains_key(&terminal_id) {
        return Err("That terminal is already running.".to_string());
    }
    sessions.insert(terminal_id.clone(), session);
    drop(sessions);

    let (output_sender, output_receiver) = mpsc::sync_channel::<Vec<u8>>(8);
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            let Ok(read) = reader.read(&mut buffer) else {
                break;
            };
            if read == 0 {
                break;
            }
            if output_sender.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });

    let output_app = app.clone();
    let output_terminal_id = terminal_id.clone();
    std::thread::spawn(move || {
        while let Ok(first) = output_receiver.recv() {
            let mut data = first;
            while data.len() < TERMINAL_OUTPUT_BATCH_BYTES {
                match output_receiver.recv_timeout(TERMINAL_OUTPUT_BATCH_DELAY) {
                    Ok(next) => data.extend(next),
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            let _ = output_app.emit(
                "vintage://terminal-output",
                TerminalOutputEvent {
                    terminal_id: output_terminal_id.clone(),
                    generation,
                    data,
                },
            );
        }
    });

    let wait_app = app.clone();
    let wait_terminal_id = terminal_id.clone();
    std::thread::spawn(move || {
        let (exit_code, signal) = match child.wait() {
            Ok(status) => (
                Some(status.exit_code()),
                status.signal().map(str::to_string),
            ),
            Err(_) => (None, None),
        };
        if let Ok(mut sessions) = wait_app.state::<TerminalRuntime>().sessions.lock() {
            if let Some(mut session) = sessions.remove(&wait_terminal_id) {
                session.killer.take();
            }
        }
        let _ = wait_app.emit(
            "vintage://terminal-exit",
            TerminalExitEvent {
                terminal_id: wait_terminal_id,
                generation,
                exit_code,
                signal,
            },
        );
    });

    Ok(TerminalInfo {
        terminal_id,
        pane_id,
        generation,
        working_directory: working_directory.to_string_lossy().into_owned(),
        shell: plan.descriptor,
        process_id,
    })
}

#[tauri::command]
pub(crate) fn terminal_write(
    state: State<'_, TerminalRuntime>,
    terminal_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    validate_terminal_id(&terminal_id)?;
    if data.len() > MAX_TERMINAL_WRITE_BYTES {
        return Err("The terminal input is too large.".to_string());
    }
    let writer = lock_sessions(&state)?
        .get(&terminal_id)
        .map(|session| Arc::clone(&session.writer))
        .ok_or_else(|| "That terminal is no longer running.".to_string())?;
    let mut writer = writer
        .lock()
        .map_err(|_| "Terminal input is unavailable.".to_string())?;
    writer
        .write_all(&data)
        .and_then(|_| writer.flush())
        .map_err(|_| "Failed to write to the terminal.".to_string())
}

#[tauri::command]
pub(crate) fn terminal_resize(
    state: State<'_, TerminalRuntime>,
    terminal_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    validate_terminal_id(&terminal_id)?;
    let size = terminal_size(cols, rows)?;
    lock_sessions(&state)?
        .get(&terminal_id)
        .ok_or_else(|| "That terminal is no longer running.".to_string())?
        .master
        .resize(size)
        .map_err(|_| "Failed to resize the terminal.".to_string())
}

#[tauri::command]
pub(crate) fn terminal_stop(
    state: State<'_, TerminalRuntime>,
    terminal_id: String,
) -> Result<(), String> {
    validate_terminal_id(&terminal_id)?;
    let session = lock_sessions(&state)?.remove(&terminal_id);
    drop(session);
    Ok(())
}

/// Renderer-derived screen state. The host validates the pane's generation
/// before relaying it to other windows; stale reports are dropped.
#[tauri::command]
pub(crate) fn agent_report_screen_state(
    app: AppHandle,
    state: State<'_, TerminalRuntime>,
    pane_id: String,
    generation: u64,
    activity: AgentActivity,
) -> Result<(), String> {
    validate_terminal_id(&pane_id)?;
    let current_generation = {
        let sessions = lock_sessions(&state)?;
        let session = sessions
            .values()
            .find(|session| session.pane_id == pane_id)
            .ok_or_else(|| "That pane is no longer running.".to_string())?;
        if session.generation != generation {
            return Err("That pane generation is stale.".to_string());
        }
        session.generation
    };
    let _ = app.emit(
        "vintage://agent-activity",
        AgentActivityEvent {
            pane_id,
            generation: current_generation,
            activity,
            source: ActivitySource::Screen,
            agent: None,
            session_id: None,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        terminal_size, validate_terminal_id, ActivitySource, MAX_TERMINAL_DIMENSION,
        TERMINAL_OUTPUT_BATCH_BYTES, TERMINAL_OUTPUT_BATCH_DELAY,
    };

    #[test]
    fn accepts_safe_terminal_identifiers() {
        assert!(validate_terminal_id("terminal-42_abcd").is_ok());
        assert!(validate_terminal_id("").is_err());
        assert!(validate_terminal_id("terminal/42").is_err());
    }

    #[test]
    fn validates_terminal_dimensions() {
        assert!(terminal_size(80, 24).is_ok());
        assert!(terminal_size(1, 24).is_err());
        assert!(terminal_size(80, MAX_TERMINAL_DIMENSION + 1).is_err());
    }

    #[test]
    fn terminal_output_batches_are_bounded_and_frame_sized() {
        assert_eq!(TERMINAL_OUTPUT_BATCH_BYTES, 32 * 1024);
        assert_eq!(
            TERMINAL_OUTPUT_BATCH_DELAY,
            std::time::Duration::from_millis(16)
        );
    }

    #[test]
    fn activity_source_serializes_to_the_frontend_contract() {
        // The renderer's AgentActivityEvent.source type is
        // "screen" | "opencode-plugin" | "runtime". OpencodePlugin must
        // serialize with the hyphen; a lowercase rename would emit
        // "opencodeplugin" and the renderer would drop the report.
        let source = ActivitySource::OpencodePlugin;
        let value = serde_json::to_string(&source).expect("serializes");
        assert_eq!(value, "\"opencode-plugin\"");
    }
}
