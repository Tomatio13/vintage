//! Local IPC for hook / plugin state reports.
//!
//! A `std::net::TcpListener` on a dedicated thread listens on 127.0.0.1 at a
//! per-launch ephemeral port. The renderer generates a 256-bit token with
//! Web Crypto and hands it to the host once via `hook_ipc_initialize`; the
//! host injects the port, token, pane id, and generation into the PTY child's
//! environment only. The renderer does not keep the token after init, and the
//! host never logs or persists it.
//!
//! Threat model: reports from another process on the same machine are the
//! attacker; a compromised WebView can already invoke Tauri commands, so the
//! token is not a defense against it.
//!
//! The listener uses `std::net` on a dedicated thread, per the plan — no
//! Tokio `net` feature and no new dependencies.

use serde::Deserialize;
use std::{
    io::{BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::terminal::{ActivitySource, AgentActivity, AgentActivityEvent, TerminalRuntime};

const READ_TIMEOUT: Duration = Duration::from_millis(500);
const WRITE_TIMEOUT: Duration = Duration::from_millis(500);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(25);
const MAX_REPORT_BYTES: usize = 16 * 1024;

/// A single hook/plugin report over the socket.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HookReport {
    #[serde(rename = "paneId")]
    pub(crate) pane_id: String,
    #[serde(rename = "generation")]
    pub(crate) generation: u64,
    #[serde(rename = "source", default)]
    pub(crate) source: String,
    // Reserved for agent-specific routing in the OpenCode plugin path.
    #[allow(dead_code)]
    #[serde(rename = "agent", default)]
    pub(crate) agent: String,
    #[serde(rename = "state", default)]
    pub(crate) state: Option<String>,
    #[serde(rename = "sessionId", default)]
    pub(crate) session_id: Option<String>,
    #[serde(rename = "authToken", default)]
    pub(crate) auth_token: Option<String>,
}

pub(crate) struct HookIpcRuntime {
    /// Listener thread handle; joined on shutdown.
    listener_thread: Mutex<Option<thread::JoinHandle<()>>>,
    /// The 256-bit token shared with the renderer.
    auth_token: Arc<Mutex<Option<Vec<u8>>>>,
    /// Bound listener port; injected into PTY child environments.
    hook_port: Mutex<Option<u16>>,
    running: Arc<AtomicBool>,
}

impl Default for HookIpcRuntime {
    fn default() -> Self {
        Self {
            listener_thread: Mutex::new(None),
            auth_token: Arc::new(Mutex::new(None)),
            hook_port: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl HookIpcRuntime {
    /// Binds the listener on the loopback interface and starts the accept
    /// loop. Returns the port the renderer connects to.
    pub(crate) fn start(&self, app: AppHandle) -> Result<u16, String> {
        if self.running.load(Ordering::Acquire) {
            return Err("Hook IPC is already running.".to_string());
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| "The hook IPC server could not start.".to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "The hook IPC server could not be configured.".to_string())?;
        let port = listener
            .local_addr()
            .map_err(|_| "The hook IPC server port is unavailable.".to_string())?
            .port();
        self.running.store(true, Ordering::Release);

        let thread_token = self.auth_token.clone();
        let thread_running = Arc::clone(&self.running);
        let handle = thread::spawn(move || {
            while thread_running.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                        let _ = handle_connection(&app, &thread_token, stream);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(ACCEPT_POLL_INTERVAL);
                    }
                    Err(_) => break,
                }
            }
        });
        *self.listener_thread.lock().unwrap() = Some(handle);
        *self.hook_port.lock().unwrap() = Some(port);
        Ok(port)
    }

    pub(crate) fn shutdown(&self) {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = self.listener_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        *self.hook_port.lock().unwrap() = None;
        *self.auth_token.lock().unwrap() = None;
    }

    pub(crate) fn set_token(&self, token: Vec<u8>) {
        *self.auth_token.lock().unwrap() = Some(token);
    }

    pub(crate) fn has_token(&self) -> bool {
        self.auth_token.lock().unwrap().is_some()
    }

    /// Environment variables for a PTY child that should report hook state.
    /// Returns empty when the IPC has not been initialized.
    pub(crate) fn child_env(&self, pane_id: &str, generation: u64) -> Vec<(String, String)> {
        let token = self.auth_token.lock().unwrap().clone();
        let port = *self.hook_port.lock().unwrap();
        let (Some(token), Some(port)) = (token, port) else {
            return Vec::new();
        };
        let token_hex = token
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        vec![
            ("XAGENT_HOOK_ENV".to_string(), "1".to_string()),
            ("XAGENT_HOOK_PORT".to_string(), port.to_string()),
            ("XAGENT_HOOK_TOKEN".to_string(), token_hex),
            ("XAGENT_PANE_ID".to_string(), pane_id.to_string()),
            ("XAGENT_GENERATION".to_string(), generation.to_string()),
        ]
    }
}

fn handle_connection(
    app: &AppHandle,
    auth_token: &Arc<Mutex<Option<Vec<u8>>>>,
    stream: TcpStream,
) -> Result<(), String> {
    let expected = auth_token
        .lock()
        .map_err(|_| "Hook IPC token is unavailable.".to_string())?
        .clone()
        .ok_or_else(|| "The hook IPC token has not been configured.".to_string())?;

    let mut reader = BufReader::new(stream.try_clone().map_err(|_| "stream clone failed")?);
    let mut buffer = vec![0_u8; MAX_REPORT_BYTES + 1];
    let read = reader
        .read(&mut buffer)
        .map_err(|_| "The hook IPC report could not be read.".to_string())?;
    if read == 0 || read > MAX_REPORT_BYTES {
        return Err("The hook IPC report exceeds the message limit.".to_string());
    }
    let report: HookReport = serde_json::from_slice(&buffer[..read])
        .map_err(|_| "The hook IPC report is malformed.".to_string())?;

    let provided = report.auth_token.clone().unwrap_or_default();
    if provided.as_bytes() != expected.as_slice() {
        return Err("The hook IPC report has an invalid token.".to_string());
    }
    if report.pane_id.is_empty() || report.pane_id.len() > 128 || report.generation == u64::MAX {
        return Err("The hook IPC report has an invalid pane reference.".to_string());
    }

    // Validate the pane's generation against the live terminal runtime;
    // stale reports are dropped.
    let terminal_state: State<'_, TerminalRuntime> = app.state();
    let (current_generation, exists) = {
        let sessions = terminal_state
            .sessions
            .lock()
            .map_err(|_| "The terminal runtime is unavailable.".to_string())?;
        match sessions
            .values()
            .find(|session| session.pane_id == report.pane_id)
        {
            Some(session) => (session.generation, true),
            None => (0, false),
        }
    };
    if !exists {
        return Err("That pane is no longer running.".to_string());
    }
    if current_generation != report.generation {
        return Err("That pane generation is stale.".to_string());
    }

    // Map the report state to the wire activity. Codex/Claude hooks report
    // only session identity (state stays None) per the plan.
    let activity = match report.state.as_deref() {
        Some("blocked") => AgentActivity::Blocked,
        Some("working") => AgentActivity::Working,
        Some("idle") => AgentActivity::Idle,
        Some("unknown") | None => AgentActivity::Unknown,
        Some(_) => return Err("The hook IPC report has an invalid state.".to_string()),
    };
    let source = match report.source.as_str() {
        "opencode-plugin" => ActivitySource::OpencodePlugin,
        _ => ActivitySource::Runtime,
    };

    let _ = app.emit(
        "vintage://agent-activity",
        AgentActivityEvent {
            pane_id: report.pane_id,
            generation: current_generation,
            activity,
            source,
            session_id: report.session_id,
        },
    );

    let mut stream = stream;
    stream
        .write_all(b"ok")
        .and_then(|_| stream.flush())
        .map_err(|_| "The hook IPC acknowledgement could not be sent.".to_string())
}

#[cfg(test)]
mod tests {
    use super::{HookIpcRuntime, HookReport};
    use std::{sync::atomic::Ordering, sync::Arc, thread, time::Duration};

    #[test]
    fn token_must_be_set_before_reports_are_accepted() {
        let runtime = HookIpcRuntime::default();
        assert!(!runtime.has_token());
    }

    #[test]
    fn set_token_stores_and_has_token_reports_it() {
        let runtime = HookIpcRuntime::default();
        runtime.set_token(vec![7_u8; 32]);
        assert!(runtime.has_token());
    }

    #[test]
    fn shutdown_signals_and_joins_the_listener_thread() {
        let runtime = HookIpcRuntime::default();
        runtime.set_token(vec![7_u8; 32]);
        *runtime.hook_port.lock().unwrap() = Some(12345);
        runtime.running.store(true, Ordering::Release);

        let running = Arc::clone(&runtime.running);
        *runtime.listener_thread.lock().unwrap() = Some(thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
        }));

        runtime.shutdown();

        assert!(!runtime.running.load(Ordering::Acquire));
        assert!(runtime.listener_thread.lock().unwrap().is_none());
        assert!(runtime.hook_port.lock().unwrap().is_none());
        assert!(!runtime.has_token());
    }

    #[test]
    fn reports_carry_the_wire_fields() {
        let report = HookReport {
            pane_id: "pane-1".to_string(),
            generation: 3,
            source: "opencode-plugin".to_string(),
            agent: "opencode".to_string(),
            state: Some("working".to_string()),
            session_id: Some("sess-9".to_string()),
            auth_token: Some("token".to_string()),
        };
        assert_eq!(report.pane_id, "pane-1");
        assert_eq!(report.generation, 3);
        assert_eq!(report.source, "opencode-plugin");
        assert_eq!(report.state.as_deref(), Some("working"));
    }
}
