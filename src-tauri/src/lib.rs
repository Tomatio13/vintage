mod file_manager;
mod hook_ipc;
mod integrations;
mod shells;
mod terminal;
mod workspaces;

use chrono::Local;
use file_manager::{
    workspace_inspect_attachment, workspace_list_directory, workspace_open_folder,
    workspace_preview_file, workspace_unwatch, workspace_watch, WorkspaceWatcherRuntime,
};
use serde::Serialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use terminal::{
    agent_report_screen_state, terminal_resize, terminal_start, terminal_stop, terminal_write,
    TerminalRuntime,
};
use workspaces::{
    HostError, LoadLayoutOutcome, WorkspaceLayoutFile, WorkspaceRootRecord, WorkspaceRuntime,
};

const MAX_ATTACHMENTS: usize = 10;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileAttachment {
    path: String,
    name: String,
    size: i64,
    mime_type: Option<String>,
}

#[derive(Default)]
struct AppUpdateRuntime {
    pending: std::sync::Mutex<Option<Update>>,
    installing: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateInfo {
    current_version: String,
    version: String,
    body: Option<String>,
    date: Option<String>,
}

impl From<&Update> for AppUpdateInfo {
    fn from(update: &Update) -> Self {
        Self {
            current_version: update.current_version.clone(),
            version: update.version.clone(),
            body: update.body.clone(),
            date: update.date.map(|date| date.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateProgress {
    stage: &'static str,
    downloaded: u64,
    total: Option<u64>,
}

#[tauri::command]
fn configure_native_titlebar(window: tauri::WebviewWindow) -> Result<Option<f64>, String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSWindow, NSWindowButton};

        const MINIMUM_TITLEBAR_HEIGHT: f64 = 40.0;

        let ns_window_ptr = window
            .ns_window()
            .map_err(|error| format!("Failed to access the native window: {error}"))?
            as *mut NSWindow;
        let ns_window = unsafe { &*ns_window_ptr };
        let window_frame = ns_window.frame();
        let native_height = window_frame.size.height - ns_window.contentLayoutRect().size.height;
        let height = native_height.max(MINIMUM_TITLEBAR_HEIGHT);

        if let Some(close_button) = ns_window.standardWindowButton(NSWindowButton::CloseButton) {
            let button_parent = unsafe { close_button.superview() };
            let titlebar_container = button_parent
                .as_ref()
                .and_then(|parent| unsafe { parent.superview() });

            if let Some(container) = titlebar_container {
                let mut container_frame = container.frame();
                container_frame.size.height = height;
                container_frame.origin.y = window_frame.size.height - height;
                container.setFrame(container_frame);

                for kind in [
                    NSWindowButton::CloseButton,
                    NSWindowButton::MiniaturizeButton,
                    NSWindowButton::ZoomButton,
                ] {
                    let Some(button) = ns_window.standardWindowButton(kind) else {
                        continue;
                    };
                    let Some(parent) = (unsafe { button.superview() }) else {
                        continue;
                    };
                    let parent_frame = parent.frame();
                    let mut button_frame = button.frame();
                    button_frame.origin.y =
                        height / 2.0 - parent_frame.origin.y - button_frame.size.height / 2.0;
                    button.setFrameOrigin(button_frame.origin);
                }
            }
        }

        return Ok((height.is_finite() && height > 0.0 && height <= 96.0).then_some(height));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(None)
    }
}

#[tauri::command]
async fn check_app_update(
    app: AppHandle,
    state: State<'_, AppUpdateRuntime>,
) -> Result<Option<AppUpdateInfo>, String> {
    if state.installing.load(Ordering::Acquire) {
        return Err("An update is already being installed.".to_string());
    }

    let update = app
        .updater()
        .map_err(|error| format!("Failed to prepare the updater: {error}"))?
        .check()
        .await
        .map_err(|error| format!("Failed to check for updates: {error}"))?;
    let info = update.as_ref().map(AppUpdateInfo::from);
    *state.pending.lock().unwrap() = update;
    Ok(info)
}

#[tauri::command]
async fn install_app_update(
    app: AppHandle,
    state: State<'_, AppUpdateRuntime>,
) -> Result<(), String> {
    if state
        .installing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("An update is already being installed.".to_string());
    }

    let result: Result<(), String> = async {
        let update = state
            .pending
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "Check for updates before installing one.".to_string())?;

        let _ = app.emit(
            "vintage://update-progress",
            AppUpdateProgress {
                stage: "downloading",
                downloaded: 0,
                total: None,
            },
        );

        let progress_app = app.clone();
        let finished_app = app.clone();
        let mut downloaded = 0_u64;
        let bytes = update
            .download(
                move |chunk_length, total| {
                    downloaded = downloaded.saturating_add(chunk_length as u64);
                    let _ = progress_app.emit(
                        "vintage://update-progress",
                        AppUpdateProgress {
                            stage: "downloading",
                            downloaded,
                            total,
                        },
                    );
                },
                move || {
                    let _ = finished_app.emit(
                        "vintage://update-progress",
                        AppUpdateProgress {
                            stage: "downloaded",
                            downloaded: 0,
                            total: None,
                        },
                    );
                },
            )
            .await
            .map_err(|error| format!("Failed to download the update: {error}"))?;

        let package_size = bytes.len() as u64;
        let _ = app.emit(
            "vintage://update-progress",
            AppUpdateProgress {
                stage: "installing",
                downloaded: package_size,
                total: Some(package_size),
            },
        );
        update
            .install(&bytes)
            .map_err(|error| format!("Failed to install the update: {error}"))?;
        Ok(())
    }
    .await;

    state.installing.store(false, Ordering::Release);
    result?;
    *state.pending.lock().unwrap() = None;
    app.restart()
}

fn inspect_attachment_paths(paths: Vec<PathBuf>) -> Result<Vec<FileAttachment>, String> {
    let mut seen = HashSet::new();
    let mut attachments = Vec::new();

    for path in paths {
        let canonical = path
            .canonicalize()
            .map_err(|_| "One of the selected files is no longer available.".to_string())?;
        if !seen.insert(canonical.clone()) {
            continue;
        }
        if attachments.len() == MAX_ATTACHMENTS {
            return Err(format!("Attach up to {MAX_ATTACHMENTS} files at a time."));
        }

        let metadata = canonical
            .metadata()
            .map_err(|_| "One of the selected files could not be inspected.".to_string())?;
        if !metadata.is_file() {
            return Err("Only files can be attached to a message.".to_string());
        }
        let size = i64::try_from(metadata.len())
            .map_err(|_| "One of the selected files is too large to attach.".to_string())?;
        let name = canonical
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| "One of the selected files has no usable name.".to_string())?;

        attachments.push(FileAttachment {
            path: canonical.to_string_lossy().into_owned(),
            name,
            size,
            mime_type: mime_guess::from_path(&canonical)
                .first_raw()
                .map(str::to_string),
        });
    }

    Ok(attachments)
}

fn shutdown_app(app: &AppHandle) {
    app.state::<hook_ipc::HookIpcRuntime>().shutdown();
    app.state::<WorkspaceWatcherRuntime>().shutdown();
    app.state::<TerminalRuntime>().shutdown();
}

fn workspace_registry_host_path(app: &AppHandle) -> Result<PathBuf, HostError> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(workspaces::WORKSPACE_REGISTRY_FILE))
        .map_err(|_| HostError::io_error("Could not access the application data directory."))
}

fn workspace_layout_host_path(app: &AppHandle) -> Result<PathBuf, HostError> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(workspaces::WORKSPACE_LAYOUT_FILE))
        .map_err(|_| HostError::io_error("Could not access the application data directory."))
}

async fn register_workspace_root(
    app: &AppHandle,
    candidate: &Path,
) -> Result<WorkspaceRootRecord, HostError> {
    let canonical = candidate.canonicalize().map_err(|_| {
        HostError::invalid_request("Could not open the selected working directory.")
    })?;
    if !canonical.is_dir() {
        return Err(HostError::invalid_request(
            "Choose a folder to use as a workspace.",
        ));
    }
    let path = canonical.to_string_lossy().into_owned();
    let registry_path = workspace_registry_host_path(app)?;
    let mut load = workspaces::load_registry(&registry_path).await?;
    if let Some(existing) = load.roots.iter().find(|root| root.path == path) {
        return Ok(existing.clone());
    }
    if load.roots.len() >= workspaces::MAX_WORKSPACES {
        return Err(HostError::invalid_request(
            "The workspace limit has been reached.",
        ));
    }
    let record = WorkspaceRootRecord {
        id: workspaces::new_workspace_id(),
        path,
        title: workspaces::title_from_path(&canonical),
        created_at: Local::now().timestamp_millis(),
    };
    load.roots.push(record.clone());
    workspaces::save_registry(&registry_path, &load.roots).await?;
    Ok(record)
}

#[tauri::command]
async fn workspace_list_roots(
    app: AppHandle,
    state: State<'_, WorkspaceRuntime>,
) -> Result<Vec<WorkspaceRootRecord>, HostError> {
    let _guard = state.lock.lock().await;
    let path = workspace_registry_host_path(&app)?;
    let load = workspaces::load_registry(&path).await?;
    if load.migrated {
        workspaces::save_registry(&path, &load.roots).await?;
    }
    Ok(load.roots)
}

#[tauri::command]
async fn workspace_choose_root(
    app: AppHandle,
    state: State<'_, WorkspaceRuntime>,
) -> Result<Option<WorkspaceRootRecord>, HostError> {
    let picked = tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Choose a workspace folder")
            .pick_folder()
    })
    .await
    .map_err(|_| HostError::unavailable("Failed to open the folder picker."))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let _guard = state.lock.lock().await;
    register_workspace_root(&app, &picked).await.map(Some)
}

#[tauri::command]
async fn workspace_add_root(
    app: AppHandle,
    state: State<'_, WorkspaceRuntime>,
    path: String,
) -> Result<WorkspaceRootRecord, HostError> {
    let _guard = state.lock.lock().await;
    register_workspace_root(&app, Path::new(&path)).await
}

#[tauri::command]
async fn workspace_remove_root(
    app: AppHandle,
    state: State<'_, WorkspaceRuntime>,
    watcher_state: State<'_, WorkspaceWatcherRuntime>,
    terminal_state: State<'_, TerminalRuntime>,
    workspace_id: String,
) -> Result<Vec<WorkspaceRootRecord>, HostError> {
    let _guard = state.lock.lock().await;
    let registry_path = workspace_registry_host_path(&app)?;
    let load = workspaces::load_registry(&registry_path).await?;
    let index = load
        .roots
        .iter()
        .position(|root| root.id == workspace_id)
        .ok_or_else(|| HostError::not_found("That workspace is not registered."))?;
    let mut roots = load.roots;
    let removed = roots.remove(index);
    workspaces::save_registry(&registry_path, &roots).await?;
    // Unregistration only: the directory and its files are never deleted.
    // File watchers for the removed root stop here; PTYs join this stop path
    // in Phase 3 when terminal start records its workspace id.
    if let Ok(canonical) = PathBuf::from(&removed.path).canonicalize() {
        watcher_state.stop_for_root(&canonical);
    }
    terminal_state.forget_workspace(&workspace_id);
    Ok(roots)
}

#[tauri::command]
async fn workspace_layout_load(
    app: AppHandle,
    state: State<'_, WorkspaceRuntime>,
) -> Result<LoadLayoutOutcome, HostError> {
    let _guard = state.lock.lock().await;
    let path = workspace_layout_host_path(&app)?;
    let outcome = workspaces::load_layout_file(&path).await;
    if matches!(outcome, LoadLayoutOutcome::Invalid { .. }) {
        state.autosave_disabled.store(true, Ordering::Release);
    }
    Ok(outcome)
}

#[tauri::command]
async fn workspace_layout_save(
    app: AppHandle,
    state: State<'_, WorkspaceRuntime>,
    layout: WorkspaceLayoutFile,
) -> Result<(), HostError> {
    let _guard = state.lock.lock().await;
    if state.autosave_disabled.load(Ordering::Acquire) {
        return Err(HostError::invalid_config(
            "Layout autosave is disabled until the damaged layout is backed up and reset.",
        ));
    }
    let path = workspace_layout_host_path(&app)?;
    workspaces::save_layout_file(&path, &layout).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LayoutResetResult {
    backup_path: Option<String>,
}

#[tauri::command]
async fn workspace_layout_backup_and_reset(
    app: AppHandle,
    state: State<'_, WorkspaceRuntime>,
) -> Result<LayoutResetResult, HostError> {
    let _guard = state.lock.lock().await;
    let path = workspace_layout_host_path(&app)?;
    let backup =
        workspaces::backup_and_reset_layout_file(&path, &workspaces::utc_timestamp_now()).await?;
    state.autosave_disabled.store(false, Ordering::Release);
    Ok(LayoutResetResult {
        backup_path: backup.map(|backup| backup.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
fn shell_list() -> Vec<shells::ShellDescriptor> {
    shells::detect_shells()
}

#[tauri::command]
fn agent_list_presets() -> Vec<terminal::AgentPresetInfo> {
    terminal::agent_preset_info()
}

#[tauri::command]
fn integration_list() -> Vec<integrations::IntegrationStatus> {
    use integrations::IntegrationAgent;
    [
        IntegrationAgent::Codex,
        IntegrationAgent::Claude,
        IntegrationAgent::Opencode,
    ]
    .into_iter()
    .map(|agent| {
        integrations::status(agent).unwrap_or_else(|message| integrations::IntegrationStatus {
            agent,
            state: integrations::IntegrationState::Conflict,
            script_path: None,
            message,
        })
    })
    .collect()
}

#[tauri::command]
fn integration_install(agent: String) -> Result<integrations::IntegrationStatus, String> {
    let agent = parse_integration_agent(&agent)?;
    integrations::install(agent)
}

#[tauri::command]
fn integration_uninstall(agent: String) -> Result<integrations::IntegrationStatus, String> {
    let agent = parse_integration_agent(&agent)?;
    integrations::uninstall(agent)
}

fn parse_integration_agent(agent: &str) -> Result<integrations::IntegrationAgent, String> {
    match agent {
        "codex" => Ok(integrations::IntegrationAgent::Codex),
        "claude" => Ok(integrations::IntegrationAgent::Claude),
        "opencode" => Ok(integrations::IntegrationAgent::Opencode),
        _ => Err("That integration is unknown.".to_string()),
    }
}

/// Renderer passes a 256-bit token (Web Crypto) exactly once; the host binds
/// the local IPC listener and returns the port. The renderer must not retain
/// the token afterward.
#[tauri::command]
fn hook_ipc_initialize(
    app: AppHandle,
    state: State<'_, hook_ipc::HookIpcRuntime>,
    token: Vec<u8>,
) -> Result<u16, String> {
    if token.len() != 32 {
        return Err("The hook IPC token must be 32 bytes.".to_string());
    }
    if state.has_token() {
        return Err("The hook IPC token has already been configured.".to_string());
    }
    state.set_token(token);
    let port = state.start(app)?;
    Ok(port)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppUpdateRuntime::default())
        .manage(TerminalRuntime::default())
        .manage(WorkspaceWatcherRuntime::default())
        .manage(WorkspaceRuntime::default())
        .manage(hook_ipc::HookIpcRuntime::default())
        .invoke_handler(tauri::generate_handler![
            configure_native_titlebar,
            check_app_update,
            install_app_update,
            workspace_list_directory,
            workspace_inspect_attachment,
            workspace_open_folder,
            workspace_preview_file,
            workspace_watch,
            workspace_unwatch,
            workspace_list_roots,
            workspace_choose_root,
            workspace_add_root,
            workspace_remove_root,
            workspace_layout_load,
            workspace_layout_save,
            workspace_layout_backup_and_reset,
            shell_list,
            agent_list_presets,
            agent_report_screen_state,
            hook_ipc_initialize,
            integration_list,
            integration_install,
            integration_uninstall,
            terminal_start,
            terminal_write,
            terminal_resize,
            terminal_stop,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            shutdown_app(app);
        }
    });
}
