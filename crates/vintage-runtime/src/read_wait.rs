//! Unix PTY readiness with an explicit cancellation descriptor.
use std::{
    io,
    os::{
        fd::{AsRawFd, BorrowedFd, OwnedFd},
        unix::net::UnixStream,
    },
};

pub(crate) struct ReadWait {
    terminal: OwnedFd,
    cancellation: UnixStream,
}

impl ReadWait {
    /// The returned peer is closed by the service to interrupt an idle reader.
    /// All descriptors are owned and close-on-exec; no child inherits the peer.
    pub(crate) fn new(terminal: BorrowedFd<'_>) -> io::Result<(Self, UnixStream)> {
        let terminal = terminal.try_clone_to_owned()?;
        let (cancellation, signal) = UnixStream::pair()?;
        Ok((
            Self {
                terminal,
                cancellation,
            },
            signal,
        ))
    }

    /// Wait without a periodic timeout. HUP/ERR on the PTY also permit a read,
    /// so buffered final output is drained before EOF/EIO is handled normally.
    pub(crate) fn wait(&self) -> io::Result<bool> {
        let mut descriptors = [
            libc::pollfd {
                fd: self.terminal.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.cancellation.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        loop {
            // SAFETY: the array is valid for its stated length and both owned
            // descriptors remain open throughout this call.
            let result =
                unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, -1) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if descriptors[1].revents != 0 {
                return Ok(false);
            }
            if descriptors[0].revents & libc::POLLNVAL != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "PTY readiness descriptor is invalid",
                ));
            }
            if descriptors[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                return Ok(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::fd::AsFd,
        sync::mpsc,
        thread,
        time::Duration,
    };

    #[test]
    fn idle_reader_waits_until_cancelled() {
        let (reader, _writer) = UnixStream::pair().unwrap();
        let (wait, cancel) = ReadWait::new(reader.as_fd()).unwrap();
        let (send, receive) = mpsc::channel();
        let worker = thread::spawn(move || send.send(wait.wait().unwrap()).unwrap());
        assert!(receive.recv_timeout(Duration::from_millis(30)).is_err());
        drop(cancel);
        assert!(!receive.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn ready_bytes_are_not_consumed_and_hangup_preserves_final_data() {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        let (wait, _cancel) = ReadWait::new(reader.as_fd()).unwrap();
        writer.write_all(b"synthetic final output").unwrap();
        drop(writer);
        assert!(wait.wait().unwrap());
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"synthetic final output");
        assert!(wait.wait().unwrap()); // EOF remains readable.
    }

    #[test]
    fn cancellation_before_wait_wins_over_ready_output() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let (wait, cancel) = ReadWait::new(reader.as_fd()).unwrap();
        writer.write_all(b"synthetic").unwrap();
        drop(cancel);
        assert!(!wait.wait().unwrap());
    }

    #[test]
    fn descriptors_are_owned_and_not_inherited_by_exec() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let (wait, cancel) = ReadWait::new(reader.as_fd()).unwrap();
        drop(reader);
        for fd in [
            wait.terminal.as_raw_fd(),
            wait.cancellation.as_raw_fd(),
            cancel.as_raw_fd(),
        ] {
            // SAFETY: each descriptor is owned by a live value above.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert!(flags >= 0);
            assert_ne!(flags & libc::FD_CLOEXEC, 0);
        }
        writer.write_all(b"synthetic").unwrap();
        assert!(wait.wait().unwrap());
    }
}
