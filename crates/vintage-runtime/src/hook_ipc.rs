//! Authenticated loopback IPC for managed agent hooks and plugins.
//!
//! The server is process-local: every launch creates a fresh 256-bit token and
//! an ephemeral loopback port. Only the native session owner receives those
//! values, which it injects into its own PTY child environment.
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    io::{BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const MAX_REPORT_BYTES: usize = 16 * 1024;
const READ_TIMEOUT: Duration = Duration::from_millis(500);
const ACCEPT_POLL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookActivity {
    pub pane: u64,
    pub generation: u64,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub state: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HookReport {
    pane_id: String,
    generation: u64,
    #[serde(default)]
    agent: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    auth_token: Option<String>,
}

pub struct HookIpc {
    token: [u8; 32],
    port: u16,
    running: Arc<AtomicBool>,
    listener: Option<JoinHandle<()>>,
    events: Arc<Mutex<mpsc::Receiver<HookActivity>>>,
    active_panes: Arc<Mutex<BTreeSet<u64>>>,
}

impl HookIpc {
    /// Starts an authenticated listener on loopback only.
    pub fn start() -> Result<Self, String> {
        let mut token = [0_u8; 32];
        getrandom::fill(&mut token)
            .map_err(|_| "Could not create Hook IPC credentials.".to_string())?;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| "The Hook IPC server could not start.".to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "The Hook IPC server could not be configured.".to_string())?;
        let port = listener
            .local_addr()
            .map_err(|_| "The Hook IPC server port is unavailable.".to_string())?
            .port();
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let expected = token_hex(&token);
        let (sender, receiver) = mpsc::sync_channel(64);
        let active_panes = Arc::new(Mutex::new(BTreeSet::new()));
        let thread_active_panes = active_panes.clone();
        let listener_thread = thread::spawn(move || {
            while thread_running.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                        let _ = accept_report(stream, &expected, &thread_active_panes, &sender);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(ACCEPT_POLL)
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            token,
            port,
            running,
            listener: Some(listener_thread),
            events: Arc::new(Mutex::new(receiver)),
            active_panes,
        })
    }

    pub fn child_env(&self, pane: u64, generation: u64, agent: &str) -> Vec<(String, String)> {
        vec![
            ("VINTAGE_HOOK_ENV".into(), "1".into()),
            ("VINTAGE_HOOK_PORT".into(), self.port.to_string()),
            ("VINTAGE_HOOK_TOKEN".into(), token_hex(&self.token)),
            ("VINTAGE_PANE_ID".into(), format!("pane-{pane}")),
            ("VINTAGE_GENERATION".into(), generation.to_string()),
            ("VINTAGE_AGENT".into(), agent.into()),
        ]
    }

    pub fn register_pane(&self, pane: u64) {
        self.active_panes
            .lock()
            .expect("hook active pane mutex poisoned")
            .insert(pane);
    }
    pub fn unregister_pane(&self, pane: u64) {
        self.active_panes
            .lock()
            .expect("hook active pane mutex poisoned")
            .remove(&pane);
    }

    /// A cloneable receiver handle. Exactly one UI owner should consume it.
    pub fn events(&self) -> Arc<Mutex<mpsc::Receiver<HookActivity>>> {
        self.events.clone()
    }

    pub fn shutdown(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}
impl Drop for HookIpc {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_report(
    stream: TcpStream,
    expected: &str,
    active_panes: &Arc<Mutex<BTreeSet<u64>>>,
    sender: &mpsc::SyncSender<HookActivity>,
) -> Result<(), String> {
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|_| "Hook IPC stream failed.".to_string())?,
    );
    let mut buffer = vec![0_u8; MAX_REPORT_BYTES + 1];
    let read = reader
        .read(&mut buffer)
        .map_err(|_| "Hook IPC report could not be read.".to_string())?;
    if read == 0 || read > MAX_REPORT_BYTES {
        return Err("Hook IPC report exceeds the message limit.".into());
    }
    let report: HookReport = serde_json::from_slice(&buffer[..read])
        .map_err(|_| "Hook IPC report is malformed.".to_string())?;
    if report.auth_token.as_deref() != Some(expected) {
        return Err("Hook IPC report has an invalid token.".into());
    }
    let pane = report
        .pane_id
        .strip_prefix("pane-")
        .and_then(|id| id.parse::<u64>().ok())
        .ok_or_else(|| "Hook IPC report has an invalid pane reference.".to_string())?;
    if !active_panes
        .lock()
        .expect("hook active pane mutex poisoned")
        .contains(&pane)
        || report.generation != 1
        || report.agent.len() > 128
        || report.session_id.as_ref().is_some_and(|id| id.len() > 512)
    {
        return Err("Hook IPC report has invalid fields.".into());
    }
    if !matches!(
        report.state.as_deref(),
        None | Some("working" | "blocked" | "idle" | "released" | "unknown")
    ) {
        return Err("Hook IPC report has an invalid state.".into());
    }
    let activity = HookActivity {
        pane,
        generation: report.generation,
        agent: (!report.agent.is_empty()).then_some(report.agent),
        session_id: report.session_id,
        state: report.state,
    };
    sender
        .send(activity)
        .map_err(|_| "Hook IPC receiver is unavailable.".to_string())?;
    let mut stream = stream;
    stream
        .write_all(b"ok")
        .and_then(|_| stream.flush())
        .map_err(|_| "Hook IPC acknowledgement failed.".into())
}

fn token_hex(token: &[u8]) -> String {
    token.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_environment_has_fresh_private_connection_data() {
        let hook = HookIpc::start().unwrap();
        let values: std::collections::BTreeMap<_, _> =
            hook.child_env(7, 1, "codex").into_iter().collect();
        assert_eq!(
            values.get("VINTAGE_PANE_ID").map(String::as_str),
            Some("pane-7")
        );
        assert_eq!(
            values.get("VINTAGE_GENERATION").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            values.get("VINTAGE_AGENT").map(String::as_str),
            Some("codex")
        );
        assert_eq!(values.get("VINTAGE_HOOK_TOKEN").map(String::len), Some(64));
    }
}
