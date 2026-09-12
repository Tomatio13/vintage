//! Native ownership of startup workers and all workspace sessions.
use crate::{hook_ipc::HookIpc, Session};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};
use vintage_core::{SessionId, TerminalSize};

pub struct Owner {
    pub startup: Option<JoinHandle<anyhow::Result<Session>>>,
    pub session: Option<Session>,
    pub error: Option<String>,
}
impl Owner {
    pub fn poll(&mut self) {
        if self.startup.as_ref().is_some_and(|task| task.is_finished()) {
            match self.startup.take().unwrap().join() {
                Ok(Ok(session)) => self.session = Some(session),
                Ok(Err(error)) => self.error = Some(error.to_string()),
                Err(_) => self.error = Some("Terminal startup worker failed".into()),
            }
        }
    }
    fn shutdown(&mut self) {
        if let Some(task) = self.startup.take() {
            match task.join() {
                Ok(Ok(session)) => self.session = Some(session),
                Ok(Err(error)) => self.error = Some(error.to_string()),
                Err(_) => self.error = Some("Terminal startup worker failed".into()),
            }
        }
        if let Some(session) = self.session.take() {
            if session.shutdown().is_err() {
                self.error = Some("Terminal shutdown failed".into());
            }
        }
    }
}

#[derive(Default)]
pub struct NativeSessions {
    owners: Mutex<BTreeMap<u64, Arc<Mutex<Owner>>>>,
    closing: Mutex<Vec<JoinHandle<Option<String>>>>,
    shutdown_guard: Mutex<()>,
    stopped: AtomicBool,
    cleanup_error: Mutex<Option<String>>,
    hook_ipc: Option<Arc<HookIpc>>,
}
impl NativeSessions {
    pub fn with_hook_ipc(hook_ipc: Arc<HookIpc>) -> Self {
        Self {
            hook_ipc: Some(hook_ipc),
            ..Self::default()
        }
    }
    /// Scheduling only; shell discovery and filesystem validation run on a worker.
    pub fn start(&self, pane: u64, root: PathBuf, shell: String) -> Arc<Mutex<Owner>> {
        self.start_with_scrollback(pane, root, shell, 1000)
    }
    pub fn start_with_scrollback(
        &self,
        pane: u64,
        root: PathBuf,
        shell: String,
        scrollback: usize,
    ) -> Arc<Mutex<Owner>> {
        let mut owners = self.owners.lock().expect("session registry mutex poisoned");
        if self.stopped.load(Ordering::Acquire)
            || (!owners.contains_key(&pane) && owners.len() >= vintage_core::workspace::MAX_PANES)
        {
            return Arc::new(Mutex::new(Owner {
                startup: None,
                session: None,
                error: Some("Terminal service is stopped or at capacity".into()),
            }));
        }
        let hook_ipc = self.hook_ipc.clone();
        if let Some(hook) = &hook_ipc {
            hook.register_pane(pane);
        }
        owners
            .entry(pane)
            .or_insert_with(|| {
                Arc::new(Mutex::new(Owner {
                    startup: Some(std::thread::spawn(move || {
                        let environment = hook_ipc
                            .as_ref()
                            .map(|hook| hook.child_env(pane, 1, ""))
                            .unwrap_or_default();
                        Session::start_with_scrollback_and_env(
                            SessionId::new(&format!("terminal-{pane}"), &format!("pane-{pane}"), 1)
                                .unwrap(),
                            &root,
                            &shell,
                            TerminalSize::new(80, 24).unwrap(),
                            scrollback,
                            environment,
                        )
                    })),
                    session: None,
                    error: None,
                }))
            })
            .clone()
    }
    /// Closing a pane must not join native workers on the UI thread.
    pub fn close(&self, pane: u64) {
        if let Some(hook) = &self.hook_ipc {
            hook.unregister_pane(pane);
        }
        let owner = self
            .owners
            .lock()
            .expect("session registry mutex poisoned")
            .remove(&pane);
        if let Some(owner) = owner {
            let mut closing = self.closing.lock().expect("closing workers mutex poisoned");
            let mut finished = Vec::new();
            let mut i = 0;
            while i < closing.len() {
                if closing[i].is_finished() {
                    finished.push(closing.swap_remove(i));
                } else {
                    i += 1;
                }
            }
            closing.push(std::thread::spawn(move || {
                let mut error = None;
                for worker in finished {
                    match worker.join() {
                        Ok(Some(message)) => error = Some(message),
                        Err(_) => error = Some("Terminal cleanup worker failed".into()),
                        Ok(None) => {}
                    }
                }
                let mut owner = owner.lock().expect("session owner mutex poisoned");
                owner.shutdown();
                owner.error.clone().or(error)
            }));
        }
    }
    /// Call on a background worker or after the application event loop returns.
    pub fn shutdown(&self) -> anyhow::Result<()> {
        let _guard = self.shutdown_guard.lock().expect("shutdown mutex poisoned");
        self.stopped.store(true, Ordering::Release);
        let owners =
            std::mem::take(&mut *self.owners.lock().expect("session registry mutex poisoned"));
        let mut error = None;
        for owner in owners.into_values() {
            let mut owner = owner.lock().expect("session owner mutex poisoned");
            owner.shutdown();
            if owner.error.is_some() {
                error = owner.error.clone();
            }
        }
        for worker in
            std::mem::take(&mut *self.closing.lock().expect("closing workers mutex poisoned"))
        {
            match worker.join() {
                Ok(Some(message)) => error = Some(message),
                Err(_) => error = Some("Terminal cleanup worker failed".into()),
                Ok(None) => {}
            }
        }
        let mut remembered = self
            .cleanup_error
            .lock()
            .expect("cleanup error mutex poisoned");
        if error.is_some() {
            *remembered = error;
        }
        if let Some(error) = remembered.as_ref() {
            anyhow::bail!("{error}");
        }
        Ok(())
    }
}

/// Resolve workspace selections on a background worker, outside GPUI callbacks.
pub fn workspace_root(path: &std::path::Path) -> anyhow::Result<PathBuf> {
    let root = path.canonicalize()?;
    anyhow::ensure!(root.is_dir(), "Workspace must be a directory");
    Ok(root)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn closing_during_startup_and_bulk_shutdown_release_every_session() {
        let registry = NativeSessions::default();
        let first = registry.start(1, PathBuf::from("."), "unix-bash-fast".into());
        let second = registry.start(2, PathBuf::from("."), "unix-bash-fast".into());
        registry.close(1);
        registry.shutdown().unwrap();
        for owner in [first, second] {
            let owner = owner.lock().unwrap();
            assert!(owner.startup.is_none());
            assert!(owner.session.is_none());
        }
        assert!(registry.owners.lock().unwrap().is_empty());
        assert!(registry.closing.lock().unwrap().is_empty());
        registry.shutdown().unwrap();
        let rejected = registry.start(3, PathBuf::from("."), "unix-bash-fast".into());
        assert!(rejected.lock().unwrap().startup.is_none());
        assert!(rejected.lock().unwrap().error.is_some());
    }
    #[test]
    fn shutdown_preserves_errors_for_final_application_check() {
        let registry = NativeSessions::default();
        registry.start(
            1,
            PathBuf::from("/nonexistent-vintage-test-root"),
            "unix-bash-fast".into(),
        );
        assert!(registry.shutdown().is_err());
        assert!(registry.shutdown().is_err());
    }
}
