//! Native PTY service. Blocking work runs on service workers.
mod integration_assets;
#[allow(dead_code)]
mod shells;

#[derive(Clone, Debug)]
pub struct IntegrationSummary {
    pub agent: String,
    pub state: String,
    pub message: String,
}

fn integration_agent(agent: &str) -> Result<integration_assets::IntegrationAgent, String> {
    match agent {
        "codex" => Ok(integration_assets::IntegrationAgent::Codex),
        "claude" => Ok(integration_assets::IntegrationAgent::Claude),
        "opencode" => Ok(integration_assets::IntegrationAgent::Opencode),
        _ => Err("That integration is unknown.".to_string()),
    }
}

fn summarize(status: integration_assets::IntegrationStatus) -> IntegrationSummary {
    IntegrationSummary {
        agent: status.agent.as_str().to_string(),
        state: format!("{:?}", status.state),
        message: status.message,
    }
}

pub fn integration_statuses() -> Vec<IntegrationSummary> {
    ["codex", "claude", "opencode"]
        .into_iter()
        .map(
            |agent| match integration_agent(agent).and_then(integration_assets::status) {
                Ok(status) => summarize(status),
                Err(message) => IntegrationSummary {
                    agent: agent.to_string(),
                    state: "Conflict".to_string(),
                    message,
                },
            },
        )
        .collect()
}

pub fn install_integration(agent: &str) -> Result<IntegrationSummary, String> {
    integration_assets::install(integration_agent(agent)?).map(summarize)
}

pub fn uninstall_integration(agent: &str) -> Result<IntegrationSummary, String> {
    integration_assets::uninstall(integration_agent(agent)?).map(summarize)
}

pub mod files;
pub mod hook_ipc;
pub mod native_sessions;
#[cfg(unix)]
mod read_wait;
pub mod settings;
mod updates;
pub use updates::Updates;

use anyhow::{anyhow, bail, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use vintage_core::{SessionId, TerminalSize, MAX_INPUT_BYTES};
use vintage_terminal::{Snapshot, Terminal};

pub fn available_shells() -> Vec<(String, String)> {
    shells::detect_shells()
        .into_iter()
        .filter(|s| s.available)
        .map(|s| (s.id, s.label))
        .collect()
}

pub fn default_shell_id() -> &'static str {
    if cfg!(windows) {
        "windows-default"
    } else {
        "unix-default"
    }
}

enum Command {
    Input(Vec<u8>),
    Resize(TerminalSize),
    Scroll(i32),
    Scrollback(usize),
    Select((usize, usize), (usize, usize)),
    ClearSelection,
}

struct Shared {
    terminal: Mutex<Terminal>,
    revision: AtomicU64,
    updates: updates::UpdateSender,
    stopped: AtomicBool,
    #[cfg(unix)]
    read_cancel: Mutex<Option<std::os::unix::net::UnixStream>>,
    exited: AtomicBool,
    error: Mutex<Option<String>>,
}
impl Shared {
    fn request_stop(&self) {
        self.stopped.store(true, Ordering::Release);
        #[cfg(unix)]
        self.read_cancel
            .lock()
            .expect("read cancellation mutex poisoned")
            .take();
        self.updates.close();
    }

    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::Release);
        self.updates.notify();
    }
    fn fail(&self, message: &str) {
        *self.error.lock().expect("error mutex poisoned") = Some(message.into());
        self.changed();
    }
}

struct Workers {
    handles: Vec<JoinHandle<()>>,
}

pub struct Session {
    id: SessionId,
    input: SyncSender<Command>,
    shared: Arc<Shared>,
    workers: Option<Workers>,
    updates: Option<Updates>,
}

impl Session {
    /// Must run on a background worker. The spike uses an explicit root and does
    /// not read the production workspace registry or persisted launch commands.
    pub fn start(id: SessionId, root: &Path, shell: &str, size: TerminalSize) -> Result<Self> {
        Self::start_with_scrollback(id, root, shell, size, 1000)
    }
    pub fn start_with_scrollback(
        id: SessionId,
        root: &Path,
        shell: &str,
        size: TerminalSize,
        scrollback: usize,
    ) -> Result<Self> {
        Self::start_with_scrollback_and_env(id, root, shell, size, scrollback, Vec::new())
    }
    pub fn start_with_scrollback_and_env(
        id: SessionId,
        root: &Path,
        shell: &str,
        size: TerminalSize,
        scrollback: usize,
        hook_environment: Vec<(String, String)>,
    ) -> Result<Self> {
        anyhow::ensure!(
            settings::SCROLLBACK.contains(&scrollback),
            "Unsupported scrollback capacity"
        );
        SessionId::new(&id.terminal, &id.pane, id.generation).map_err(|e| anyhow!(e))?;
        TerminalSize::new(size.columns, size.rows).map_err(|e| anyhow!(e))?;
        let root = root
            .canonicalize()
            .context("Cannot resolve the working directory")?;
        if !root.is_dir() {
            bail!("Working directory must be a directory");
        }
        let resolved =
            shells::resolve_shell(&shells::detect_shells(), shell).map_err(|e| anyhow!(e))?;
        shells::validate_shell_executable(Path::new(&resolved.descriptor.executable))
            .map_err(|e| anyhow!(e))?;
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .context("Cannot create a PTY")?;
        // Duplicated Unix PTY handles share this nonblocking flag. Workers can
        // honor cancellation even when a descendant keeps the slave open.
        #[cfg(unix)]
        if let Some(fd) = pair.master.as_raw_fd() {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(std::io::Error::last_os_error())
                    .context("Cannot configure PTY cancellation");
            }
        }
        #[cfg(unix)]
        let (read_wait, read_cancel) = {
            let fd = pair
                .master
                .as_raw_fd()
                .context("PTY has no readiness descriptor")?;
            // SAFETY: pair.master owns this live descriptor for the whole borrow.
            read_wait::ReadWait::new(unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) })
                .context("Cannot configure PTY readiness")?
        };
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("Cannot open PTY reader")?;
        let mut writer = pair
            .master
            .take_writer()
            .context("Cannot open PTY writer")?;
        let mut command = CommandBuilder::new(&resolved.descriptor.executable);
        clear_inherited_hook_env(&mut command);
        command.args(&resolved.args);
        command.cwd(root);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        for (key, value) in &resolved.env {
            command.env(key, value);
        }
        for (key, value) in hook_environment {
            command.env(key, value);
        }
        let mut child = pair
            .slave
            .spawn_command(command)
            .context("Cannot launch the shell")?;
        drop(pair.slave);
        let master = Arc::new(Mutex::new(pair.master));
        let write_master = master.clone();
        let (updates, receiver) = updates::channel();
        let shared = Arc::new(Shared {
            terminal: Mutex::new({
                let mut terminal = Terminal::new(size);
                terminal.set_scrollback(scrollback);
                terminal
            }),
            revision: AtomicU64::new(1),
            updates,
            stopped: AtomicBool::new(false),
            #[cfg(unix)]
            read_cancel: Mutex::new(Some(read_cancel)),
            exited: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        // At most 32 * 64 KiB queued input. Output is parsed synchronously by the
        // reader; slowing the parser applies kernel PTY backpressure without loss.
        let (input, receive) = mpsc::sync_channel(32);
        let write_shared = shared.clone();
        let write_handle = thread::spawn(move || {
            while !write_shared.stopped.load(Ordering::Acquire) {
                let command = match receive.recv_timeout(Duration::from_millis(25)) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let result: Result<()> = (|| {
                    match command {
                        Command::Input(bytes) => {
                            let mut offset = 0;
                            while offset < bytes.len()
                                && !write_shared.stopped.load(Ordering::Acquire)
                            {
                                match writer.write(&bytes[offset..]) {
                                    Ok(0) => bail!("PTY writer closed"),
                                    Ok(count) => offset += count,
                                    Err(error)
                                        if error.kind() == std::io::ErrorKind::Interrupted =>
                                    {
                                        continue
                                    }
                                    Err(error)
                                        if error.kind() == std::io::ErrorKind::WouldBlock =>
                                    {
                                        thread::sleep(Duration::from_millis(2))
                                    }
                                    Err(error) => return Err(error.into()),
                                }
                            }
                            writer.flush()?;
                        }
                        Command::Resize(size) => {
                            // Lock across the resize so bytes cannot be parsed with mismatched dimensions.
                            let mut terminal = write_shared
                                .terminal
                                .lock()
                                .expect("terminal mutex poisoned");
                            write_master
                                .lock()
                                .expect("PTY mutex poisoned")
                                .resize(pty_size(size))?;
                            terminal.resize(size);
                        }
                        Command::Scrollback(lines) => write_shared
                            .terminal
                            .lock()
                            .expect("terminal mutex poisoned")
                            .set_scrollback(lines),
                        Command::Scroll(lines) => write_shared
                            .terminal
                            .lock()
                            .expect("terminal mutex poisoned")
                            .scroll(lines),
                        Command::Select(start, end) => write_shared
                            .terminal
                            .lock()
                            .expect("terminal mutex poisoned")
                            .select(start, end),
                        Command::ClearSelection => write_shared
                            .terminal
                            .lock()
                            .expect("terminal mutex poisoned")
                            .clear_selection(),
                    }
                    Ok(())
                })();
                if result.is_err() {
                    write_shared.fail("Terminal write or resize failed");
                    break;
                }
                write_shared.changed();
            }
            // Dropping the writer/master also releases the native PTY handles.
        });
        let read_shared = shared.clone();
        let replies = input.clone();
        let read_handle = thread::spawn(move || {
            let mut buffer = [0; 8192];
            while !read_shared.stopped.load(Ordering::Acquire) {
                let count = match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        #[cfg(unix)]
                        match read_wait.wait() {
                            Ok(true) => {}
                            Ok(false) => break,
                            Err(_) => {
                                read_shared.fail("Terminal readiness wait failed");
                                break;
                            }
                        }
                        #[cfg(not(unix))]
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    // Linux PTYs report EIO after the slave closes.
                    Err(error) if cfg!(target_os = "linux") && error.raw_os_error() == Some(5) => {
                        break
                    }
                    Err(_) => {
                        read_shared.fail("Terminal read failed");
                        break;
                    }
                };
                let responses = read_shared
                    .terminal
                    .lock()
                    .expect("terminal mutex poisoned")
                    .feed(&buffer[..count]);
                read_shared.changed();
                for bytes in responses {
                    if replies.send(Command::Input(bytes)).is_err() {
                        return;
                    }
                }
            }
            read_shared.changed();
        });
        let wait_shared = shared.clone();
        let wait_handle = thread::spawn(move || {
            // This is the only owner that reaps or signals the child. Never keep
            // a raw-PID killer alive after wait() could allow that PID to be reused.
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {}
                    Err(_) => {
                        wait_shared.fail("Cannot inspect the shell process");
                        break;
                    }
                }
                if wait_shared.stopped.load(Ordering::Acquire) {
                    #[cfg(unix)]
                    {
                        let foreground = master
                            .lock()
                            .expect("PTY mutex poisoned")
                            .process_group_leader();
                        if let Some(group) = foreground
                            .filter(|group| *group > 0 && *group != unsafe { libc::getpgrp() })
                        {
                            // The terminal's foreground job may have its own process group.
                            if unsafe { libc::kill(-group, libc::SIGKILL) } < 0
                                && std::io::Error::last_os_error().raw_os_error()
                                    != Some(libc::ESRCH)
                            {
                                wait_shared.fail("Cannot terminate the foreground job");
                            }
                        }
                        if let Some(pid) = child.process_id() {
                            // The child is still unreaped, so its PID cannot be reused.
                            if unsafe { libc::kill(-(pid as i32), libc::SIGKILL) } < 0
                                && std::io::Error::last_os_error().raw_os_error()
                                    != Some(libc::ESRCH)
                            {
                                wait_shared.fail("Cannot terminate the shell process group");
                            }
                        }
                    }
                    #[cfg(windows)]
                    if child.kill().is_err() {
                        wait_shared.fail("Cannot terminate the shell");
                    }
                    if child.wait().is_err() {
                        wait_shared.fail("Cannot reap the shell process");
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            wait_shared.exited.store(true, Ordering::Release);
            wait_shared.changed();
        });
        Ok(Self {
            id,
            updates: Some(receiver),
            input,
            shared,
            workers: Some(Workers {
                handles: vec![write_handle, read_handle, wait_handle],
            }),
        })
    }

    /// The session has exactly one coalescing UI listener.
    pub fn take_updates(&mut self) -> Option<Updates> {
        self.updates.take()
    }

    pub fn id(&self) -> &SessionId {
        &self.id
    }
    pub fn revision(&self) -> u64 {
        self.shared.revision.load(Ordering::Acquire)
    }
    pub fn exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }
    pub fn error(&self) -> Option<String> {
        self.shared.error.try_lock().ok()?.clone()
    }
    pub fn snapshot(&self) -> Option<Snapshot> {
        let mut snapshot = self.shared.terminal.try_lock().ok()?.snapshot();
        if self.exited() {
            // Final output can arrive after child exit. Mask interactive modes
            // on every snapshot instead of resetting the still-draining parser.
            snapshot.mouse = Default::default();
            snapshot.focus_reporting = false;
            snapshot.cursor = None;
        }
        Some(snapshot)
    }
    pub fn selected_text(&self) -> Option<String> {
        self.shared.terminal.try_lock().ok()?.selection_text()
    }
    pub fn bottom_text(&self) -> Option<String> {
        Some(self.shared.terminal.try_lock().ok()?.bottom_text())
    }

    fn enqueue(&self, command: Command) -> Result<()> {
        if self.exited() && matches!(&command, Command::Input(_) | Command::Resize(_)) {
            bail!("The terminal session has ended");
        }
        self.input
            .try_send(command)
            .map_err(|_| anyhow!("Terminal input queue is busy or closed; retry the action"))
    }
    pub fn write(&self, id: &SessionId, bytes: Vec<u8>) -> Result<()> {
        if id != &self.id {
            bail!("Stale terminal session");
        }
        if bytes.len() > MAX_INPUT_BYTES {
            bail!("Terminal input exceeds the size limit");
        }
        self.enqueue(Command::Input(bytes))
    }
    pub fn resize(&self, size: TerminalSize) -> Result<()> {
        TerminalSize::new(size.columns, size.rows).map_err(|e| anyhow!(e))?;
        self.enqueue(Command::Resize(size))
    }
    pub fn set_scrollback(&self, lines: usize) -> Result<()> {
        anyhow::ensure!(
            settings::SCROLLBACK.contains(&lines),
            "Unsupported scrollback capacity"
        );
        self.enqueue(Command::Scrollback(lines))
    }
    pub fn scroll(&self, lines: i32) -> Result<()> {
        self.enqueue(Command::Scroll(lines))
    }
    pub fn select(&self, start: (usize, usize), end: (usize, usize)) -> Result<()> {
        self.enqueue(Command::Select(start, end))
    }
    pub fn clear_selection(&self) -> Result<()> {
        self.enqueue(Command::ClearSelection)
    }

    /// Explicit synchronous shutdown for a background owner or tests.
    pub fn shutdown(mut self) -> Result<()> {
        self.stop_workers()
    }
    fn stop_workers(&mut self) -> Result<()> {
        self.shared.request_stop();
        if let Some(workers) = self.workers.take() {
            let mut failed = false;
            for handle in workers.handles {
                failed |= handle.join().is_err();
            }
            if failed {
                bail!("A terminal worker panicked during shutdown");
            }
            if let Some(error) = self.error() {
                bail!("{error}");
            }
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Never join a process or worker from GPUI's main thread.
        if let Some(workers) = self.workers.take() {
            let shared = self.shared.clone();
            shared.request_stop();
            thread::spawn(move || {
                for handle in workers.handles {
                    if handle.join().is_err() {
                        shared.fail("A terminal worker panicked during shutdown");
                    }
                }
            });
        }
    }
}

const HOOK_ENVIRONMENT: [&str; 6] = [
    "VINTAGE_HOOK_ENV",
    "VINTAGE_HOOK_PORT",
    "VINTAGE_HOOK_TOKEN",
    "VINTAGE_PANE_ID",
    "VINTAGE_GENERATION",
    "VINTAGE_AGENT",
];

fn clear_inherited_hook_env(command: &mut CommandBuilder) {
    // The spike has no Hook server. Never let a shell launched from production
    // accidentally report back using its parent's authentication and pane ID.
    for key in HOOK_ENVIRONMENT {
        command.env_remove(key);
    }
}

fn pty_size(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.columns,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn session_notifies_resize_and_closes_its_single_listener() {
        use std::{
            future::Future,
            pin::pin,
            task::{Context, Poll, Wake, Waker},
        };
        struct WakeThread(thread::Thread);
        impl Wake for WakeThread {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        let mut session = Session::start(
            SessionId::new("updates", "pane", 1).unwrap(),
            Path::new("."),
            "unix-bash-fast",
            TerminalSize::new(80, 24).unwrap(),
        )
        .unwrap();
        let mut updates = session.take_updates().unwrap();
        assert!(session.take_updates().is_none());
        let waker = Waker::from(Arc::new(WakeThread(thread::current())));
        let mut cx = Context::from_waker(&waker);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        session.resize(TerminalSize::new(90, 30).unwrap()).unwrap();
        loop {
            let mut next = pin!(updates.next());
            loop {
                match next.as_mut().poll(&mut cx) {
                    Poll::Ready(true) => break,
                    Poll::Ready(false) => panic!("Listener closed before resize"),
                    Poll::Pending => {
                        let remaining =
                            deadline.saturating_duration_since(std::time::Instant::now());
                        assert!(!remaining.is_zero(), "Resize notification was lost");
                        thread::park_timeout(remaining);
                    }
                }
            }
            if session
                .snapshot()
                .is_some_and(|snapshot| snapshot.size.rows == 30)
            {
                break;
            }
        }
        session.shutdown().unwrap();
        // One final coalesced refresh may precede the closed indication.
        let final_update = {
            let mut next = pin!(updates.next());
            next.as_mut().poll(&mut cx)
        };
        match final_update {
            Poll::Ready(true) => assert_eq!(pin!(updates.next()).poll(&mut cx), Poll::Ready(false)),
            Poll::Ready(false) => {}
            Poll::Pending => panic!("Shutdown left listener waiting"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn real_pty_input_resize_stale_generation_and_shutdown() {
        let id = SessionId::new("test-terminal", "test-pane", 1).unwrap();
        let session = Session::start(
            id.clone(),
            Path::new("."),
            "unix-bash-fast",
            TerminalSize::new(80, 24).unwrap(),
        )
        .unwrap();
        let mut stale = id.clone();
        stale.generation += 1;
        assert!(session.write(&stale, b"echo stale\r".to_vec()).is_err());
        assert!(session.write(&id, vec![b'x'; MAX_INPUT_BYTES + 1]).is_err());
        assert!(session
            .resize(TerminalSize {
                rows: 0,
                columns: 80
            })
            .is_err());
        session.resize(TerminalSize::new(90, 30).unwrap()).unwrap();
        session
            .write(
                &id,
                b"printf '\\033[2J\\033[HVT_%s' 'SYNTHETIC_OK'\r".to_vec(),
            )
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if session
                .bottom_text()
                .is_some_and(|text| text.contains("VT_SYNTHETIC_OK"))
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Synthetic terminal output did not arrive"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(session.snapshot().unwrap().size.rows, 30);
        session.shutdown().unwrap();
    }
    #[cfg(unix)]
    fn wait_for(session: &Session, needle: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !session
            .bottom_text()
            .is_some_and(|text| text.contains(needle))
        {
            assert!(
                std::time::Instant::now() < deadline,
                "Synthetic terminal output did not arrive"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[cfg(unix)]
    #[test]
    fn natural_exit_drains_final_output_and_shutdown_does_not_signal_reaped_pid() {
        let session = Session::start(
            SessionId::new("drain", "pane", 1).unwrap(),
            Path::new("."),
            "unix-bash-fast",
            TerminalSize::new(80, 24).unwrap(),
        )
        .unwrap();
        session.write(session.id(), b"unset HISTFILE; for ((i=0;i<2500;i++)); do printf 'synthetic output %s\\n' \"$i\"; done; printf 'FINAL_%s\\n' 'OK'; exit\r".to_vec()).unwrap();
        wait_for(&session, "FINAL_OK");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !session.exited() {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(session.write(session.id(), b"no".to_vec()).is_err());
        session.shutdown().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn exited_session_keeps_scrollback_and_selection_available() {
        let session = Session::start(
            SessionId::new("exited-selection", "pane", 1).unwrap(),
            Path::new("."),
            "unix-bash-fast",
            TerminalSize::new(80, 24).unwrap(),
        )
        .unwrap();
        session.write(session.id(), b"unset HISTFILE; for ((i=0;i<100;i++)); do printf 'row %s\\n' \"$i\"; done; printf 'END_%s\\n' 'MARKER'; exit\r".to_vec()).unwrap();
        wait_for(&session, "END_MARKER");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !session.exited() {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(session.write(session.id(), b"no".to_vec()).is_err());
        assert!(session.resize(TerminalSize::new(90, 30).unwrap()).is_err());
        // Simulate modes in late final output after the supervisor reports exit.
        // The parser must retain its state while snapshots remain inspectable.
        {
            let mut terminal = session.shared.terminal.lock().unwrap();
            terminal.feed(b"\x1b[?1003h\x1b[?1006h\x1b[?1004h\x1b[?25h");
            let raw = terminal.snapshot();
            assert!(raw.mouse.enabled());
            assert!(raw.focus_reporting);
            assert!(raw.cursor.is_some());
        }
        let snapshot = session.snapshot().unwrap();
        assert!(!snapshot.mouse.enabled());
        assert!(!snapshot.focus_reporting);
        assert!(snapshot.cursor.is_none());
        session.scroll(10).unwrap();
        session.select((0, 0), (0, 4)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while session
            .snapshot()
            .is_none_or(|snapshot| snapshot.display_offset != 10)
            || !session
                .selected_text()
                .is_some_and(|text| text.contains("row"))
        {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(session
            .selected_text()
            .is_some_and(|text| text.contains("row")));
        session.clear_selection().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while session.selected_text().is_some() {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        session.shutdown().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_cancels_a_shell_ignoring_hangup() {
        let session = Session::start(
            SessionId::new("shutdown", "pane", 1).unwrap(),
            Path::new("."),
            "unix-bash-fast",
            TerminalSize::new(80, 24).unwrap(),
        )
        .unwrap();
        session
            .write(
                session.id(),
                b"trap '' HUP; printf 'READY_%s' 'TO_STOP'; while :; do sleep 1; done\r".to_vec(),
            )
            .unwrap();
        wait_for(&session, "READY_TO_STOP");
        let started = std::time::Instant::now();
        session.shutdown().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn repeated_start_and_immediate_close_join_all_workers() {
        for generation in 1..=5 {
            Session::start(
                SessionId::new("restart", "pane", generation).unwrap(),
                Path::new("."),
                "unix-bash-fast",
                TerminalSize::new(80, 24).unwrap(),
            )
            .unwrap()
            .shutdown()
            .unwrap();
        }
    }
    #[test]
    fn preview_cannot_inherit_production_hook_authority() {
        let mut command = CommandBuilder::new("synthetic-program");
        for key in HOOK_ENVIRONMENT {
            command.env(key, "synthetic-test-value");
        }
        command.env("VINTAGE_TEST_UNRELATED", "preserved");
        clear_inherited_hook_env(&mut command);
        for key in HOOK_ENVIRONMENT {
            assert!(command.get_env(key).is_none());
        }
        assert_eq!(
            command.get_env("VINTAGE_TEST_UNRELATED"),
            Some(std::ffi::OsStr::new("preserved"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn real_windows_pty_input_resize_and_shutdown() {
        let session = Session::start(
            SessionId::new("windows-test", "pane", 1).unwrap(),
            Path::new("."),
            "windows-default",
            TerminalSize::new(80, 24).unwrap(),
        )
        .unwrap();
        session.resize(TerminalSize::new(90, 30).unwrap()).unwrap();
        session
            .write(
                session.id(),
                b"Write-Output ('VT_' + 'SYNTHETIC_OK')\r".to_vec(),
            )
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !session
            .bottom_text()
            .is_some_and(|text| text.contains("VT_SYNTHETIC_OK"))
        {
            assert!(
                std::time::Instant::now() < deadline,
                "Synthetic Windows terminal output did not arrive"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(session.snapshot().unwrap().size.rows, 30);
        session.shutdown().unwrap();
    }
}
