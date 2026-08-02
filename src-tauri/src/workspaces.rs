//! Workspace registry and layout persistence.
//!
//! After the Grok conversation era, the registry (`working-directories.json`)
//! is the source of truth for workspace boundaries, and the layout file
//! (`workspace-layouts.json`) persists placement only. Runtime state — PTY
//! ids, generations, scrollback, hook tokens, activity — is never persisted.
//!
//! Load validation rejects the whole file on any violation: there is no
//! partial recovery. A damaged file is left untouched on disk, autosave
//! stops, and the user can explicitly back it up and reset to an empty
//! version 1 layout.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

pub(crate) const WORKSPACE_REGISTRY_FILE: &str = "working-directories.json";
pub(crate) const WORKSPACE_LAYOUT_FILE: &str = "workspace-layouts.json";
pub(crate) const LAYOUT_SCHEMA_VERSION: u32 = 1;

pub(crate) const MAX_WORKSPACES: usize = 64;
pub(crate) const MAX_TABS_PER_WORKSPACE: usize = 64;
pub(crate) const MAX_PANES_PER_TAB: usize = 64;
pub(crate) const MAX_SPLIT_DEPTH: usize = 16;
pub(crate) const MIN_SPLIT_RATIO: f64 = 0.2;
pub(crate) const MAX_SPLIT_RATIO: f64 = 0.8;
pub(crate) const MAX_TITLE_CODE_POINTS: usize = 128;
pub(crate) const MAX_ID_LENGTH: usize = 128;
pub(crate) const MAX_PROGRAM_BYTES: usize = 32 * 1024;
pub(crate) const MAX_ARGS: usize = 256;
pub(crate) const MAX_ARG_BYTES: usize = 32 * 1024;
pub(crate) const MAX_ARGV_BYTES: usize = 64 * 1024;

/// Structured host error shared by the new workspace commands.
///
/// Serialized as `{ code, message }` with snake_case codes, matching the
/// minimal Tauri wire contract in the implementation plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HostErrorCode {
    InvalidRequest,
    NotFound,
    /// Reserved by the wire contract; first used by integration management.
    #[allow(dead_code)]
    Conflict,
    Unavailable,
    IoError,
    InvalidConfig,
    /// Reserved by the wire contract; first used by hook IPC generations.
    #[allow(dead_code)]
    StaleGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostError {
    pub(crate) code: HostErrorCode,
    pub(crate) message: String,
}

impl HostError {
    pub(crate) fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: HostErrorCode::InvalidRequest,
            message: message.into(),
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: HostErrorCode::NotFound,
            message: message.into(),
        }
    }

    pub(crate) fn invalid_config(message: impl Into<String>) -> Self {
        Self {
            code: HostErrorCode::InvalidConfig,
            message: message.into(),
        }
    }

    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: HostErrorCode::Unavailable,
            message: message.into(),
        }
    }

    pub(crate) fn io_error(message: impl Into<String>) -> Self {
        Self {
            code: HostErrorCode::IoError,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for HostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for HostError {}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// A registered workspace root. `id` is the only handle the renderer ever
/// uses; `path` is owned by the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceRootRecord {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegistryFile {
    pub(crate) version: u32,
    pub(crate) roots: Vec<WorkspaceRootRecord>,
}

/// Legacy record written by the Grok-era app (`[{ path, createdAt }]`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyWorkspaceRecord {
    path: String,
    created_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawRegistry {
    Versioned(RegistryFile),
    PathList(Vec<String>),
    LegacyList(Vec<LegacyWorkspaceRecord>),
}

pub(crate) struct RegistryLoad {
    pub(crate) roots: Vec<WorkspaceRootRecord>,
    /// True when the on-disk shape predates version 1 and must be rewritten.
    pub(crate) migrated: bool,
}

/// Parses registry bytes in any known shape and migrates legacy records by
/// assigning ids. Callers persist the result when `migrated` is true.
pub(crate) fn migrate_registry_bytes(
    bytes: &[u8],
    generate_id: &mut dyn FnMut() -> String,
) -> Result<RegistryLoad, HostError> {
    let trimmed = trim_bom(bytes);
    if trimmed.is_empty() {
        return Ok(RegistryLoad {
            roots: Vec::new(),
            migrated: false,
        });
    }
    let raw: RawRegistry = serde_json::from_slice(trimmed)
        .map_err(|_| HostError::invalid_config("Workspace registry is damaged."))?;
    match raw {
        RawRegistry::Versioned(file) => {
            if file.version != LAYOUT_SCHEMA_VERSION {
                return Err(HostError::invalid_config(
                    "Workspace registry has an unsupported version.",
                ));
            }
            validate_registry_roots(&file.roots)?;
            Ok(RegistryLoad {
                roots: file.roots,
                migrated: false,
            })
        }
        RawRegistry::PathList(paths) => {
            let mut roots = Vec::new();
            for path in paths {
                roots.push(legacy_record(path, None, generate_id)?);
            }
            dedupe_by_path(&mut roots);
            validate_registry_roots(&roots)?;
            Ok(RegistryLoad {
                roots,
                migrated: true,
            })
        }
        RawRegistry::LegacyList(records) => {
            let mut roots = Vec::new();
            for record in records {
                roots.push(legacy_record(record.path, record.created_at, generate_id)?);
            }
            dedupe_by_path(&mut roots);
            validate_registry_roots(&roots)?;
            Ok(RegistryLoad {
                roots,
                migrated: true,
            })
        }
    }
}

fn legacy_record(
    path: String,
    created_at: Option<i64>,
    generate_id: &mut dyn FnMut() -> String,
) -> Result<WorkspaceRootRecord, HostError> {
    if path.is_empty() || path.chars().any(is_forbidden_value_char) {
        return Err(HostError::invalid_config(
            "Workspace registry contains an unusable path.",
        ));
    }
    let title = title_from_path(Path::new(&path));
    Ok(WorkspaceRootRecord {
        id: generate_id(),
        path,
        title,
        created_at: created_at.unwrap_or(0),
    })
}

fn dedupe_by_path(roots: &mut Vec<WorkspaceRootRecord>) {
    let mut seen = BTreeSet::new();
    roots.retain(|root| seen.insert(root.path.clone()));
}

fn validate_registry_roots(roots: &[WorkspaceRootRecord]) -> Result<(), HostError> {
    if roots.len() > MAX_WORKSPACES {
        return Err(HostError::invalid_config(
            "Workspace registry exceeds the supported number of workspaces.",
        ));
    }
    let mut ids = BTreeSet::new();
    for root in roots {
        if !is_valid_identifier(&root.id) || !ids.insert(root.id.clone()) {
            return Err(HostError::invalid_config(
                "Workspace registry contains an invalid or duplicate id.",
            ));
        }
        if root.path.is_empty() || root.path.chars().any(is_forbidden_value_char) {
            return Err(HostError::invalid_config(
                "Workspace registry contains an unusable path.",
            ));
        }
        if !is_valid_title(&root.title) {
            return Err(HostError::invalid_config(
                "Workspace registry contains a title that is too long.",
            ));
        }
    }
    Ok(())
}

pub(crate) async fn load_registry(path: &Path) -> Result<RegistryLoad, HostError> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(RegistryLoad {
                roots: Vec::new(),
                migrated: false,
            });
        }
        Err(_) => return Err(HostError::io_error("Workspace registry could not be read.")),
    };
    let mut generator = id_generator();
    migrate_registry_bytes(&bytes, &mut generator)
}

pub(crate) async fn save_registry(
    path: &Path,
    roots: &[WorkspaceRootRecord],
) -> Result<(), HostError> {
    validate_registry_roots(roots)?;
    let file = RegistryFile {
        version: LAYOUT_SCHEMA_VERSION,
        roots: roots.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&file)
        .map_err(|_| HostError::io_error("Workspace registry could not be encoded."))?;
    atomic_write(path, &bytes).await
}

/// Derives a display title from the last path component.
pub(crate) fn title_from_path(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// Layout persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum PaneLayout {
    Leaf {
        pane_id: String,
    },
    Split {
        direction: SplitDirection,
        ratio: f64,
        first: Box<PaneLayout>,
        second: Box<PaneLayout>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AgentPreset {
    Grok,
    Codex,
    Claude,
    Opencode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PaneLaunchSpec {
    Shell {
        shell_id: String,
    },
    Agent {
        preset: AgentPreset,
        shell_id: String,
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume_session_id: Option<String>,
    },
    Custom {
        program: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaneDefinition {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) shell_id: String,
    pub(crate) agent_kind: Option<AgentPreset>,
    pub(crate) launch: PaneLaunchSpec,
    pub(crate) working_directory: Option<String>,
    pub(crate) resume_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentTabState {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) layout: PaneLayout,
    pub(crate) selected_pane_id: String,
    pub(crate) panes: Vec<PaneDefinition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceState {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) tabs: Vec<AgentTabState>,
    pub(crate) selected_tab_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLayoutFile {
    pub(crate) version: u32,
    pub(crate) workspaces: Vec<WorkspaceState>,
}

pub(crate) fn empty_layout_file() -> WorkspaceLayoutFile {
    WorkspaceLayoutFile {
        version: LAYOUT_SCHEMA_VERSION,
        workspaces: Vec::new(),
    }
}

/// Outcome of loading the layout file. `Invalid` leaves the original file
/// untouched and disables autosave until the user backs up and resets.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum LoadLayoutOutcome {
    Ok { layout: WorkspaceLayoutFile },
    Empty,
    Invalid { reason: String },
}

pub(crate) async fn load_layout_file(path: &Path) -> LoadLayoutOutcome {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return LoadLayoutOutcome::Empty,
        Err(_) => {
            return LoadLayoutOutcome::Invalid {
                reason: "Workspace layout could not be read.".to_string(),
            };
        }
    };
    match serde_json::from_slice::<WorkspaceLayoutFile>(trim_bom(&bytes)) {
        Ok(file) => match validate_layout_file(&file) {
            Ok(()) => LoadLayoutOutcome::Ok { layout: file },
            Err(error) => LoadLayoutOutcome::Invalid {
                reason: error.message,
            },
        },
        Err(_) => LoadLayoutOutcome::Invalid {
            reason: "Workspace layout is damaged.".to_string(),
        },
    }
}

pub(crate) async fn save_layout_file(
    path: &Path,
    file: &WorkspaceLayoutFile,
) -> Result<(), HostError> {
    validate_layout_file(file)?;
    let bytes = serde_json::to_vec_pretty(file)
        .map_err(|_| HostError::io_error("Workspace layout could not be encoded."))?;
    atomic_write(path, &bytes).await
}

/// Renames the damaged layout file next to itself with a UTC timestamp suffix
/// and writes a fresh empty version 1 file. Returns the backup path, or None
/// when there was no original file to back up.
pub(crate) async fn backup_and_reset_layout_file(
    path: &Path,
    utc_timestamp: &str,
) -> Result<Option<PathBuf>, HostError> {
    let exists = tokio::fs::try_exists(path)
        .await
        .map_err(|_| HostError::io_error("Workspace layout could not be inspected."))?;
    let backup = if exists {
        let backup = invalid_backup_path(path, utc_timestamp);
        tokio::fs::rename(path, &backup)
            .await
            .map_err(|_| HostError::io_error("Workspace layout could not be backed up."))?;
        Some(backup)
    } else {
        None
    };
    let empty = empty_layout_file();
    save_layout_file(path, &empty).await?;
    Ok(backup)
}

pub(crate) fn invalid_backup_path(original: &Path, utc_timestamp: &str) -> PathBuf {
    let file_name = original
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace-layouts".to_string());
    let backup_name = format!("{file_name}.invalid-{utc_timestamp}.json");
    match original.parent() {
        Some(parent) => parent.join(backup_name),
        None => PathBuf::from(backup_name),
    }
}

pub(crate) fn utc_timestamp_now() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Full-file validation. Any single violation rejects the whole file; there
/// is no partial recovery.
pub(crate) fn validate_layout_file(file: &WorkspaceLayoutFile) -> Result<(), HostError> {
    if file.version != LAYOUT_SCHEMA_VERSION {
        return Err(HostError::invalid_config(
            "Workspace layout has an unsupported version.",
        ));
    }
    if file.workspaces.len() > MAX_WORKSPACES {
        return Err(HostError::invalid_config(
            "Workspace layout exceeds the supported number of workspaces.",
        ));
    }
    let mut workspace_ids = BTreeSet::new();
    for workspace in &file.workspaces {
        validate_workspace_state(workspace)?;
        if !workspace_ids.insert(workspace.id.clone()) {
            return Err(HostError::invalid_config(
                "Workspace layout contains a duplicate workspace id.",
            ));
        }
    }
    Ok(())
}

fn validate_workspace_state(workspace: &WorkspaceState) -> Result<(), HostError> {
    if !is_valid_identifier(&workspace.id) {
        return Err(HostError::invalid_config(
            "Workspace layout contains an invalid workspace id.",
        ));
    }
    if workspace.path.is_empty() || workspace.path.chars().any(is_forbidden_value_char) {
        return Err(HostError::invalid_config(
            "Workspace layout contains an unusable workspace path.",
        ));
    }
    if !is_valid_title(&workspace.title) {
        return Err(HostError::invalid_config(
            "Workspace layout contains a workspace title that is too long.",
        ));
    }
    if workspace.tabs.len() > MAX_TABS_PER_WORKSPACE {
        return Err(HostError::invalid_config(
            "Workspace layout exceeds the supported number of tabs.",
        ));
    }
    let mut tab_ids = BTreeSet::new();
    for tab in &workspace.tabs {
        validate_tab_state(tab)?;
        if !tab_ids.insert(tab.id.clone()) {
            return Err(HostError::invalid_config(
                "Workspace layout contains a duplicate tab id.",
            ));
        }
    }
    if !workspace.tabs.is_empty() && !tab_ids.contains(&workspace.selected_tab_id) {
        return Err(HostError::invalid_request(
            "Workspace layout references a missing tab.",
        ));
    }
    Ok(())
}

fn validate_tab_state(tab: &AgentTabState) -> Result<(), HostError> {
    if !is_valid_identifier(&tab.id) {
        return Err(HostError::invalid_config(
            "Workspace layout contains an invalid tab id.",
        ));
    }
    if !is_valid_title(&tab.title) {
        return Err(HostError::invalid_config(
            "Workspace layout contains a tab title that is too long.",
        ));
    }
    let leaf_ids = validate_layout_tree(&tab.layout)?;
    if leaf_ids.len() > MAX_PANES_PER_TAB {
        return Err(HostError::invalid_config(
            "Workspace layout exceeds the supported number of panes per tab.",
        ));
    }
    if !leaf_ids.contains(&tab.selected_pane_id) {
        return Err(HostError::invalid_request(
            "Workspace layout references a missing pane.",
        ));
    }
    let mut definition_ids = BTreeSet::new();
    for pane in &tab.panes {
        validate_pane_definition(pane)?;
        if !definition_ids.insert(pane.id.clone()) {
            return Err(HostError::invalid_config(
                "Workspace layout contains a duplicate pane definition.",
            ));
        }
    }
    if definition_ids != leaf_ids {
        return Err(HostError::invalid_request(
            "Workspace layout panes and split tree disagree.",
        ));
    }
    Ok(())
}

fn validate_pane_definition(pane: &PaneDefinition) -> Result<(), HostError> {
    if !is_valid_identifier(&pane.id) {
        return Err(HostError::invalid_config(
            "Workspace layout contains an invalid pane id.",
        ));
    }
    if !is_valid_title(&pane.title) {
        return Err(HostError::invalid_config(
            "Workspace layout contains a pane title that is too long.",
        ));
    }
    if pane.shell_id.is_empty()
        || !is_valid_identifier(&pane.shell_id)
        || pane.shell_id.chars().any(is_forbidden_value_char)
    {
        return Err(HostError::invalid_config(
            "Workspace layout contains an invalid shell reference.",
        ));
    }
    if let Some(directory) = &pane.working_directory {
        if directory.chars().any(is_forbidden_value_char) {
            return Err(HostError::invalid_config(
                "Workspace layout contains an unusable pane directory.",
            ));
        }
    }
    if let Some(session_id) = &pane.resume_session_id {
        if session_id.is_empty() || session_id.chars().any(is_forbidden_value_char) {
            return Err(HostError::invalid_config(
                "Workspace layout contains an unusable session reference.",
            ));
        }
    }
    validate_launch_spec(&pane.launch)
}

fn validate_launch_spec(launch: &PaneLaunchSpec) -> Result<(), HostError> {
    match launch {
        PaneLaunchSpec::Shell { shell_id } => {
            if shell_id.is_empty() || shell_id.chars().any(is_forbidden_value_char) {
                return Err(HostError::invalid_request(
                    "Launch specification references an invalid shell.",
                ));
            }
        }
        PaneLaunchSpec::Agent {
            shell_id,
            args,
            resume_session_id,
            ..
        } => {
            if shell_id.is_empty() || shell_id.chars().any(is_forbidden_value_char) {
                return Err(HostError::invalid_request(
                    "Launch specification references an invalid shell.",
                ));
            }
            validate_argument_list(args)?;
            if let Some(session_id) = resume_session_id {
                if session_id.is_empty() || session_id.chars().any(is_forbidden_value_char) {
                    return Err(HostError::invalid_request(
                        "Launch specification contains an unusable session reference.",
                    ));
                }
            }
        }
        PaneLaunchSpec::Custom { program, args } => {
            if program.is_empty()
                || program.len() > MAX_PROGRAM_BYTES
                || program.chars().any(is_forbidden_value_char)
            {
                return Err(HostError::invalid_request(
                    "Launch specification contains an invalid program.",
                ));
            }
            validate_argument_list(args)?;
        }
    }
    Ok(())
}

fn validate_argument_list(args: &[String]) -> Result<(), HostError> {
    if args.len() > MAX_ARGS {
        return Err(HostError::invalid_request(
            "Launch specification contains too many arguments.",
        ));
    }
    let mut total = 0usize;
    for arg in args {
        if arg.len() > MAX_ARG_BYTES || arg.contains('\0') {
            return Err(HostError::invalid_request(
                "Launch specification contains an unusable argument.",
            ));
        }
        total = total.saturating_add(arg.len());
    }
    if total > MAX_ARGV_BYTES {
        return Err(HostError::invalid_request(
            "Launch specification arguments exceed the supported total size.",
        ));
    }
    Ok(())
}

/// Validates the split tree and returns the set of leaf pane ids. Recursion
/// stops at the depth limit, and serde_json caps parse nesting, so hostile
/// input cannot exhaust the stack.
fn validate_layout_tree(layout: &PaneLayout) -> Result<BTreeSet<String>, HostError> {
    let mut leaf_ids = BTreeSet::new();
    collect_validated_leaves(layout, 1, &mut leaf_ids)?;
    Ok(leaf_ids)
}

fn collect_validated_leaves(
    node: &PaneLayout,
    level: usize,
    leaf_ids: &mut BTreeSet<String>,
) -> Result<(), HostError> {
    match node {
        PaneLayout::Leaf { pane_id } => {
            if !is_valid_identifier(pane_id) {
                return Err(HostError::invalid_config(
                    "Workspace layout contains an invalid pane id.",
                ));
            }
            if !leaf_ids.insert(pane_id.clone()) {
                return Err(HostError::invalid_config(
                    "Workspace layout contains a duplicate pane id.",
                ));
            }
            Ok(())
        }
        PaneLayout::Split {
            direction: _,
            ratio,
            first,
            second,
        } => {
            if level >= MAX_SPLIT_DEPTH {
                return Err(HostError::invalid_config(
                    "Workspace layout exceeds the supported split depth.",
                ));
            }
            if !ratio.is_finite() || *ratio < MIN_SPLIT_RATIO || *ratio > MAX_SPLIT_RATIO {
                return Err(HostError::invalid_config(
                    "Workspace layout contains a split ratio outside the supported range.",
                ));
            }
            collect_validated_leaves(first, level + 1, leaf_ids)?;
            collect_validated_leaves(second, level + 1, leaf_ids)
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Long-lived state for workspace commands: a write guard for registry and
/// layout mutations, and the autosave-disable flag raised by invalid loads.
#[derive(Default)]
pub(crate) struct WorkspaceRuntime {
    pub(crate) lock: Mutex<()>,
    pub(crate) autosave_disabled: AtomicBool,
}

pub(crate) fn is_valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_ID_LENGTH
        && value
            .chars()
            .all(|character| !character.is_whitespace() && !character.is_control())
}

fn is_valid_title(value: &str) -> bool {
    value.chars().count() <= MAX_TITLE_CODE_POINTS
}

fn is_forbidden_value_char(character: char) -> bool {
    character == '\0'
}

fn trim_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)
}

/// Writes to a temporary file in the same directory, then renames it over
/// the target so readers never observe a partial file. Windows cannot rename
/// over an existing file, so the target is removed first there.
pub(crate) async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), HostError> {
    let parent = path
        .parent()
        .ok_or_else(|| HostError::io_error("Workspace data location is unavailable."))?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|_| HostError::io_error("Workspace data location is unavailable."))?;
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|_| HostError::io_error("Workspace data could not be written."))?;

    #[cfg(target_os = "windows")]
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err(HostError::io_error("Workspace data could not be replaced.")),
    }

    tokio::fs::rename(&temporary, path)
        .await
        .map_err(|_| HostError::io_error("Workspace data could not be saved."))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Std-only UUID-shaped ids
//
// The plan assigns UUIDs at migration time. To avoid a new dependency (and
// any lockfile change), ids are v4-shaped UUID strings assembled from a
// SplitMix64 stream seeded with the wall clock, the process id, and a global
// counter. Collisions are not a concern at registry scale; tests inject their
// own generator for determinism.
// ---------------------------------------------------------------------------

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn fresh() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        let seed = nanos
            ^ (u64::from(std::process::id()).wrapping_mul(0x9E37_79B9))
            ^ ID_COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }
}

pub(crate) fn new_workspace_id() -> String {
    uuid_v4_shaped(&mut SplitMix64::fresh())
}

pub(crate) fn id_generator() -> impl FnMut() -> String {
    let mut rng = SplitMix64::fresh();
    move || uuid_v4_shaped(&mut rng)
}

fn uuid_v4_shaped(rng: &mut SplitMix64) -> String {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&rng.next_u64().to_le_bytes());
    bytes[8..].copy_from_slice(&rng.next_u64().to_le_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

/// Synchronous registry load for callers already on a blocking thread.
pub(crate) fn load_registry_blocking(path: &Path) -> Result<RegistryLoad, HostError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(RegistryLoad {
                roots: Vec::new(),
                migrated: false,
            });
        }
        Err(_) => return Err(HostError::io_error("Workspace registry could not be read.")),
    };
    let mut generator = id_generator();
    migrate_registry_bytes(&bytes, &mut generator)
}

#[cfg(test)]
mod tests {
    use super::{
        atomic_write, backup_and_reset_layout_file, empty_layout_file, id_generator,
        invalid_backup_path, load_layout_file, load_registry, migrate_registry_bytes,
        save_layout_file, save_registry, title_from_path, validate_layout_file, AgentPreset,
        AgentTabState, HostError, LoadLayoutOutcome, PaneDefinition, PaneLaunchSpec, PaneLayout,
        SplitDirection, WorkspaceLayoutFile, WorkspaceRootRecord, WorkspaceState,
        LAYOUT_SCHEMA_VERSION, MAX_PANES_PER_TAB, MAX_SPLIT_DEPTH,
    };
    use std::{fs, path::Path};

    fn temporary_file(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "vintage-workspaces-{label}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after the epoch")
                .as_nanos()
        ));
        path
    }

    fn fixed_generator() -> impl FnMut() -> String {
        let mut counter = 0u32;
        move || {
            counter += 1;
            format!("id-{counter:08x}")
        }
    }

    fn sample_leaf(pane_id: &str) -> PaneLayout {
        PaneLayout::Leaf {
            pane_id: pane_id.to_string(),
        }
    }

    fn sample_pane(pane_id: &str) -> PaneDefinition {
        PaneDefinition {
            id: pane_id.to_string(),
            title: "Pane".to_string(),
            shell_id: "ubuntu-default".to_string(),
            agent_kind: None,
            launch: PaneLaunchSpec::Shell {
                shell_id: "ubuntu-default".to_string(),
            },
            working_directory: None,
            resume_session_id: None,
        }
    }

    #[test]
    fn pane_launch_spec_uses_camel_case_fields() {
        let shell = PaneLaunchSpec::Shell {
            shell_id: "unix-default".to_string(),
        };
        let value = serde_json::to_value(&shell).expect("launch spec should serialize");
        assert_eq!(
            value,
            serde_json::json!({ "type": "shell", "shellId": "unix-default" })
        );
        let decoded: PaneLaunchSpec =
            serde_json::from_value(value).expect("camelCase launch spec should deserialize");
        assert_eq!(decoded, shell);

        let agent = PaneLaunchSpec::Agent {
            preset: AgentPreset::Codex,
            shell_id: "unix-default".to_string(),
            args: vec!["--help".to_string()],
            resume_session_id: Some("session-1".to_string()),
        };
        let value = serde_json::to_value(&agent).expect("agent launch should serialize");
        assert_eq!(value["shellId"], "unix-default");
        assert_eq!(value["resumeSessionId"], "session-1");
        let decoded: PaneLaunchSpec =
            serde_json::from_value(value).expect("camelCase agent launch should deserialize");
        assert_eq!(decoded, agent);
    }

    fn sample_tab(tab_id: &str) -> AgentTabState {
        AgentTabState {
            id: tab_id.to_string(),
            title: "agents".to_string(),
            layout: sample_leaf("pane-1"),
            selected_pane_id: "pane-1".to_string(),
            panes: vec![sample_pane("pane-1")],
        }
    }

    fn sample_workspace(workspace_id: &str) -> WorkspaceState {
        WorkspaceState {
            id: workspace_id.to_string(),
            path: "/home/user/project".to_string(),
            title: "project".to_string(),
            tabs: vec![sample_tab("tab-1")],
            selected_tab_id: "tab-1".to_string(),
        }
    }

    #[test]
    fn derives_titles_from_the_last_path_component() {
        assert_eq!(title_from_path(Path::new("/home/user/project")), "project");
        assert_eq!(title_from_path(Path::new("/")), "/");
        // Backslash paths only parse as multi-segment on Windows itself.
        #[cfg(windows)]
        assert_eq!(title_from_path(Path::new("C:\\Users\\me\\repo")), "repo");
    }

    #[test]
    fn migrates_legacy_object_records_and_assigns_ids() {
        let legacy =
            br#"[{"path":"/home/user/a","createdAt":111},{"path":"/home/user/b","createdAt":222}]"#;
        let mut generator = fixed_generator();
        let load = migrate_registry_bytes(legacy, &mut generator).expect("legacy must parse");
        assert!(load.migrated);
        assert_eq!(load.roots.len(), 2);
        assert_eq!(load.roots[0].id, "id-00000001");
        assert_eq!(load.roots[0].path, "/home/user/a");
        assert_eq!(load.roots[0].title, "a");
        assert_eq!(load.roots[0].created_at, 111);
        assert_eq!(load.roots[1].created_at, 222);
    }

    #[test]
    fn migrates_plain_path_arrays() {
        let legacy = br#"["/home/user/a","/home/user/b","/home/user/a"]"#;
        let mut generator = fixed_generator();
        let load = migrate_registry_bytes(legacy, &mut generator).expect("path array must parse");
        assert!(load.migrated);
        assert_eq!(load.roots.len(), 2, "duplicate paths collapse");
        assert_eq!(load.roots[0].title, "a");
    }

    #[test]
    fn accepts_versioned_registry_without_migration() {
        let file = serde_json::json!({
            "version": 1,
            "roots": [{
                "id": "root-1",
                "path": "/home/user/a",
                "title": "a",
                "createdAt": 5,
            }],
        });
        let bytes = serde_json::to_vec(&file).expect("sample serializes");
        let mut generator = fixed_generator();
        let load = migrate_registry_bytes(&bytes, &mut generator).expect("versioned must parse");
        assert!(!load.migrated);
        assert_eq!(load.roots[0].id, "root-1");
    }

    #[test]
    fn rejects_unknown_registry_versions_and_damaged_json() {
        let mut generator = fixed_generator();
        let unknown = br#"{"version":2,"roots":[]}"#;
        assert!(migrate_registry_bytes(unknown, &mut generator).is_err());
        let damaged = br#"{"version":1,"roots":["#;
        assert!(migrate_registry_bytes(damaged, &mut generator).is_err());
    }

    #[tokio::test]
    async fn registry_round_trip_preserves_records() {
        let path = temporary_file("roundtrip");
        let roots = vec![WorkspaceRootRecord {
            id: "root-1".to_string(),
            path: "/home/user/project".to_string(),
            title: "project".to_string(),
            created_at: 42,
        }];
        save_registry(&path, &roots).await.expect("save succeeds");
        let load = load_registry(&path).await.expect("load succeeds");
        assert!(!load.migrated);
        assert_eq!(load.roots, roots);
        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn layout_round_trip_is_identical() {
        let path = temporary_file("layout-roundtrip");
        let file = WorkspaceLayoutFile {
            version: LAYOUT_SCHEMA_VERSION,
            workspaces: vec![sample_workspace("ws-1")],
        };
        save_layout_file(&path, &file).await.expect("save succeeds");
        match load_layout_file(&path).await {
            LoadLayoutOutcome::Ok { layout } => assert_eq!(layout, file),
            other => panic!("expected ok outcome, got {other:?}"),
        }
        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn missing_layout_file_loads_as_empty() {
        let path = temporary_file("missing");
        assert_eq!(load_layout_file(&path).await, LoadLayoutOutcome::Empty);
    }

    #[tokio::test]
    async fn damaged_layout_file_is_left_untouched() {
        let path = temporary_file("damaged");
        let damaged = b"{\"version\":1,\"workspaces\":";
        fs::write(&path, damaged).expect("seed damaged file");
        match load_layout_file(&path).await {
            LoadLayoutOutcome::Invalid { reason } => {
                assert!(reason.to_lowercase().contains("damaged"));
            }
            other => panic!("expected invalid outcome, got {other:?}"),
        }
        assert_eq!(fs::read(&path).expect("file still readable"), damaged);
        assert!(
            save_layout_file(&path, &empty_layout_file()).await.is_ok(),
            "explicit reset path stays available"
        );
        let backup = invalid_backup_path(&path, "20260801T000000Z");
        fs::remove_file(&path).ok();
        fs::remove_file(backup).ok();
    }

    #[test]
    fn unknown_layout_version_is_rejected() {
        let file = WorkspaceLayoutFile {
            version: 2,
            workspaces: vec![],
        };
        let error = validate_layout_file(&file).expect_err("version 2 must be rejected");
        assert_eq!(
            error,
            HostError::invalid_config("Workspace layout has an unsupported version.")
        );
    }

    #[test]
    fn layout_reference_mismatches_are_rejected() {
        let mut workspace = sample_workspace("ws-1");
        workspace.selected_tab_id = "missing-tab".to_string();
        let file = WorkspaceLayoutFile {
            version: LAYOUT_SCHEMA_VERSION,
            workspaces: vec![workspace.clone()],
        };
        assert!(validate_layout_file(&file).is_err());

        let mut workspace = sample_workspace("ws-1");
        workspace.tabs[0].selected_pane_id = "missing-pane".to_string();
        let file = WorkspaceLayoutFile {
            version: LAYOUT_SCHEMA_VERSION,
            workspaces: vec![workspace.clone()],
        };
        assert!(validate_layout_file(&file).is_err());

        let mut workspace = sample_workspace("ws-1");
        workspace.tabs[0].panes.clear();
        let file = WorkspaceLayoutFile {
            version: LAYOUT_SCHEMA_VERSION,
            workspaces: vec![workspace],
        };
        assert!(validate_layout_file(&file).is_err());
    }

    #[test]
    fn split_depth_limit_is_enforced() {
        let mut layout = sample_leaf("deepest");
        for index in 0..MAX_SPLIT_DEPTH {
            layout = PaneLayout::Split {
                direction: SplitDirection::Horizontal,
                ratio: 0.5,
                first: Box::new(sample_leaf(&format!("side-{index}"))),
                second: Box::new(layout),
            };
        }
        // MAX_SPLIT_DEPTH nested splits push the deepest leaf one past the limit.
        let tab = AgentTabState {
            id: "tab-1".to_string(),
            title: "agents".to_string(),
            layout,
            selected_pane_id: "deepest".to_string(),
            panes: (0..=MAX_SPLIT_DEPTH)
                .map(|index| {
                    let pane_id = if index == MAX_SPLIT_DEPTH {
                        "deepest".to_string()
                    } else {
                        format!("side-{index}")
                    };
                    sample_pane(&pane_id)
                })
                .collect(),
        };
        let file = WorkspaceLayoutFile {
            version: LAYOUT_SCHEMA_VERSION,
            workspaces: vec![WorkspaceState {
                id: "ws-1".to_string(),
                path: "/tmp/project".to_string(),
                title: "project".to_string(),
                tabs: vec![tab],
                selected_tab_id: "tab-1".to_string(),
            }],
        };
        assert!(validate_layout_file(&file).is_err());
    }

    #[test]
    fn launch_spec_limits_are_enforced() {
        let mut pane = sample_pane("pane-1");
        pane.launch = PaneLaunchSpec::Custom {
            program: "bash".to_string(),
            args: vec!["embedded\0nul".to_string()],
        };
        let mut tab = sample_tab("tab-1");
        tab.panes = vec![pane];
        let file = WorkspaceLayoutFile {
            version: LAYOUT_SCHEMA_VERSION,
            workspaces: vec![WorkspaceState {
                id: "ws-1".to_string(),
                path: "/tmp/project".to_string(),
                title: "project".to_string(),
                tabs: vec![tab.clone()],
                selected_tab_id: "tab-1".to_string(),
            }],
        };
        assert!(validate_layout_file(&file).is_err());

        let mut pane = sample_pane("pane-1");
        pane.launch = PaneLaunchSpec::Agent {
            preset: AgentPreset::Codex,
            shell_id: "ubuntu-default".to_string(),
            args: vec!["x".to_string(); super::MAX_ARGS + 1],
            resume_session_id: None,
        };
        let mut tab = tab;
        tab.panes = vec![pane];
        let file = WorkspaceLayoutFile {
            version: LAYOUT_SCHEMA_VERSION,
            workspaces: vec![WorkspaceState {
                id: "ws-1".to_string(),
                path: "/tmp/project".to_string(),
                title: "project".to_string(),
                tabs: vec![tab],
                selected_tab_id: "tab-1".to_string(),
            }],
        };
        assert!(validate_layout_file(&file).is_err());
    }

    #[test]
    fn pane_count_limit_is_enforced() {
        let mut leaves = Vec::new();
        let mut definitions = Vec::new();
        for index in 0..=MAX_PANES_PER_TAB {
            let pane_id = format!("pane-{index}");
            leaves.push(sample_leaf(&pane_id));
            definitions.push(sample_pane(&pane_id));
        }
        let mut layout = leaves[0].clone();
        for leaf in leaves.into_iter().skip(1) {
            layout = PaneLayout::Split {
                direction: SplitDirection::Vertical,
                ratio: 0.5,
                first: Box::new(layout),
                second: Box::new(leaf),
            };
        }
        let tab = AgentTabState {
            id: "tab-1".to_string(),
            title: "agents".to_string(),
            layout,
            selected_pane_id: "pane-0".to_string(),
            panes: definitions,
        };
        let file = WorkspaceLayoutFile {
            version: LAYOUT_SCHEMA_VERSION,
            workspaces: vec![WorkspaceState {
                id: "ws-1".to_string(),
                path: "/tmp/project".to_string(),
                title: "project".to_string(),
                tabs: vec![tab],
                selected_tab_id: "tab-1".to_string(),
            }],
        };
        assert!(validate_layout_file(&file).is_err());
    }

    #[tokio::test]
    async fn atomic_write_leaves_no_temporary_file_behind() {
        let path = temporary_file("atomic");
        atomic_write(&path, b"{}").await.expect("write succeeds");
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn backup_and_reset_renames_the_original_and_writes_empty_v1() {
        let path = temporary_file("backup-reset");
        fs::write(&path, b"not json").expect("seed damaged file");
        let backup = backup_and_reset_layout_file(&path, "20260801T000000Z")
            .await
            .expect("reset succeeds")
            .expect("original file must be backed up");
        assert_eq!(backup, invalid_backup_path(&path, "20260801T000000Z"));
        assert_eq!(fs::read(&backup).expect("backup exists"), b"not json");
        match load_layout_file(&path).await {
            LoadLayoutOutcome::Ok { layout } => assert_eq!(layout, empty_layout_file()),
            other => panic!("expected ok outcome after reset, got {other:?}"),
        }
        fs::remove_file(&path).ok();
        fs::remove_file(&backup).ok();
    }

    #[tokio::test]
    async fn backup_and_reset_without_an_original_writes_empty_v1() {
        let path = temporary_file("backup-reset-empty");
        let backup = backup_and_reset_layout_file(&path, "20260801T000001Z")
            .await
            .expect("reset succeeds");
        assert_eq!(backup, None);
        match load_layout_file(&path).await {
            LoadLayoutOutcome::Ok { layout } => assert_eq!(layout, empty_layout_file()),
            other => panic!("expected ok outcome after reset, got {other:?}"),
        }
        fs::remove_file(&path).ok();
    }

    #[test]
    fn host_error_codes_match_the_wire_contract() {
        use super::HostErrorCode;
        let cases = [
            (HostErrorCode::InvalidRequest, "invalid_request"),
            (HostErrorCode::NotFound, "not_found"),
            (HostErrorCode::Conflict, "conflict"),
            (HostErrorCode::Unavailable, "unavailable"),
            (HostErrorCode::IoError, "io_error"),
            (HostErrorCode::InvalidConfig, "invalid_config"),
            (HostErrorCode::StaleGeneration, "stale_generation"),
        ];
        for (code, expected) in cases {
            let error = HostError {
                code,
                message: "example".to_string(),
            };
            let value = serde_json::to_value(&error).expect("host error serializes");
            assert_eq!(value["code"], expected);
            assert_eq!(value["message"], "example");
        }
    }

    #[test]
    fn generated_ids_are_uuid_v4_shaped_and_unique() {
        let mut generator = id_generator();
        let first = generator();
        let second = generator();
        assert_ne!(first, second);
        for id in [first, second] {
            assert_eq!(id.len(), 36);
            assert_eq!(&id[14..15], "4", "version nibble");
            assert!(
                matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'),
                "variant nibble"
            );
        }
    }
}
