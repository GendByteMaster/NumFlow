//! Restricted named-pipe transport and ownership handshake for the `numflow-input` helper.
//!
//! The helper exposes only the typed pointer commands from [`crate::input_protocol`]. The pipe is
//! local-only, single-instance per interactive session, and both peers verify the other process id
//! and executable path before pointer commands are accepted.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use thiserror::Error;

use crate::input_protocol::{ProtocolError, RejectReason};

/// File name of the helper executable, installed next to `numflow.exe` by the MSI package.
pub const HELPER_FILE_NAME: &str = "numflow-input.exe";
/// File name of the main application executable that owns the helper.
pub const APPLICATION_FILE_NAME: &str = "numflow.exe";
/// Delay before the application retries a helper that failed after it had been connected.
pub const RECONNECT_COOLDOWN: Duration = Duration::from_secs(2);

/// Failure modes of the helper transport and service.
#[derive(Debug, Error)]
pub enum InputServiceError {
    #[error("failed to create the numflow-input named pipe: {0}")]
    PipeCreate(String),
    #[error("failed to connect to the numflow-input helper pipe: {0}")]
    Connect(String),
    #[error("numflow-input peer verification failed: {0}")]
    PeerVerification(String),
    #[error("numflow-input helper is not installed next to NumFlow")]
    HelperMissing,
    #[error("failed to start the numflow-input helper: {0}")]
    Spawn(String),
    #[error("numflow-input helper pipe I/O failed: {0}")]
    Io(String),
    #[error("numflow-input helper protocol violation: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("the numflow-input helper rejected the request: {0}")]
    Rejected(#[from] RejectReason),
}

/// Returns the per-session helper pipe name for a Windows session id.
#[must_use]
pub fn pipe_name_for_session(session_id: u32) -> String {
    format!(r"\\.\pipe\numflow-input-{session_id}")
}

/// Returns whether a resolved peer executable is pinned to the `NumFlow` installation directory.
#[must_use]
pub fn peer_path_pinned(
    own_executable: Option<&Path>,
    peer_executable: Option<&str>,
    expected_peer_file: &str,
) -> bool {
    let Some(own) = own_executable else {
        return false;
    };
    let Some(peer) = peer_executable else {
        return false;
    };

    let peer_path = PathBuf::from(peer);
    let own_directory = own.parent().map_or(String::new(), normalize_path_text);
    let peer_directory = peer_path
        .parent()
        .map_or(String::new(), normalize_path_text);
    let peer_file = peer_path
        .file_name()
        .map_or(String::new(), |name| name.to_string_lossy().to_lowercase());

    own.parent().is_some()
        && peer_path.file_name().is_some()
        && peer_file == expected_peer_file.to_lowercase()
        && peer_directory == own_directory
}

/// Returns whether another helper connection attempt may start at `now`.
#[must_use]
pub fn reconnect_allowed(
    last_attempt: Option<std::time::Instant>,
    now: std::time::Instant,
    cooldown: Duration,
) -> bool {
    last_attempt.is_none_or(|previous| now.saturating_duration_since(previous) >= cooldown)
}

fn normalize_path_text(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

#[cfg(windows)]
pub use windows_impl::run_input_helper;
#[cfg(windows)]
pub(crate) use windows_impl::{HelperClient, connect_or_spawn_helper};

#[cfg(windows)]
mod windows_impl {
    use std::{
        os::windows::process::CommandExt,
        path::PathBuf,
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    use numflow_core::PointerBackend;
    use windows::{
        Win32::{
            Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE},
            Storage::FileSystem::{
                CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAGS_AND_ATTRIBUTES,
                FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
            },
            System::{
                Pipes::{
                    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
                    GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
                    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
                },
                RemoteDesktop::ProcessIdToSessionId,
                Threading::{
                    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32,
                    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
                },
            },
        },
        core::{Error as WindowsError, HRESULT, PCWSTR, PWSTR},
    };

    use crate::{
        diagnostics::current_process_ui_access,
        input_protocol::{
            ButtonAction, FrameDecoder, HEADER_LEN, MAX_PAYLOAD_LEN, Message, RejectReason,
        },
        pointer::DirectWindowsPointer,
    };

    use super::{
        APPLICATION_FILE_NAME, HELPER_FILE_NAME, InputServiceError, peer_path_pinned,
        pipe_name_for_session,
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CONNECT_TOTAL_TIMEOUT: Duration = Duration::from_secs(2);
    const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(40);
    const PIPE_BUFFER_SIZE: u32 = 4_096;
    const LIVENESS_NONCE: u32 = 0x4E46_4950;

    struct OwnedHandle(HANDLE);

    impl OwnedHandle {
        const fn get(&self) -> HANDLE {
            self.0
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: this type owns the handle and closes it exactly once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    impl std::fmt::Debug for OwnedHandle {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.debug_tuple("OwnedHandle").field(&self.0).finish()
        }
    }

    fn current_process_id() -> u32 {
        // SAFETY: GetCurrentProcessId has no preconditions and only returns the caller's PID.
        unsafe { GetCurrentProcessId() }
    }

    fn current_session_id() -> Result<u32, InputServiceError> {
        let mut session_id = 0_u32;
        // SAFETY: the process id is valid for the current process and output points to a local u32.
        unsafe { ProcessIdToSessionId(current_process_id(), &raw mut session_id) }
            .map_err(|error| InputServiceError::Io(error.to_string()))?;
        Ok(session_id)
    }

    fn helper_pipe_name() -> Result<String, InputServiceError> {
        Ok(pipe_name_for_session(current_session_id()?))
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn own_executable_path() -> Result<PathBuf, InputServiceError> {
        std::env::current_exe().map_err(|error| InputServiceError::Io(error.to_string()))
    }

    fn helper_executable_path() -> Result<PathBuf, InputServiceError> {
        let current = own_executable_path()?;
        let directory = current.parent().ok_or(InputServiceError::HelperMissing)?;
        Ok(directory.join(HELPER_FILE_NAME))
    }

    fn process_image_path(process_id: u32) -> Option<String> {
        // SAFETY: only query access is requested; the handle is closed before return.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;
            let mut buffer = vec![0_u16; 1_024];
            let mut size = u32::try_from(buffer.len()).ok()?;
            let result = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &raw mut size,
            );
            let _ = CloseHandle(process);
            result.ok()?;
            let length = usize::try_from(size).ok()?.min(buffer.len());
            Some(String::from_utf16_lossy(&buffer[..length]))
        }
    }

    fn spawn_helper_process() -> Result<(), InputServiceError> {
        let helper = helper_executable_path()?;
        if !helper.is_file() {
            return Err(InputServiceError::HelperMissing);
        }

        Command::new(&helper)
            .arg("--input-service")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|error| InputServiceError::Spawn(error.to_string()))
    }

    fn open_client_pipe_once() -> Result<OwnedHandle, InputServiceError> {
        let name = wide(&helper_pipe_name()?);
        // SAFETY: the path is a valid NUL-terminated UTF-16 string and no handles are inherited.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }
        .map_err(|error| InputServiceError::Connect(error.to_string()))?;
        Ok(OwnedHandle(handle))
    }

    fn read_frame(pipe: HANDLE, decoder: &mut FrameDecoder) -> Result<Message, InputServiceError> {
        let mut chunk = [0_u8; HEADER_LEN + MAX_PAYLOAD_LEN];
        loop {
            if let Some(message) = decoder.next_frame()? {
                return Ok(message);
            }

            let mut bytes_read = 0_u32;
            // SAFETY: the handle is a connected synchronous pipe and the output buffer is valid.
            unsafe { ReadFile(pipe, Some(&mut chunk), Some(&raw mut bytes_read), None) }
                .map_err(|error| InputServiceError::Io(error.to_string()))?;
            let bytes_read = usize::try_from(bytes_read).unwrap_or(0);
            if bytes_read == 0 {
                return Err(InputServiceError::Io(
                    "the peer closed the helper pipe".to_owned(),
                ));
            }
            decoder.push(&chunk[..bytes_read]);
        }
    }

    fn write_frame(pipe: HANDLE, message: Message) -> Result<(), InputServiceError> {
        let frame = message.encode();
        let mut bytes_written = 0_u32;
        // SAFETY: the handle is a connected synchronous pipe and the frame remains alive.
        unsafe { WriteFile(pipe, Some(&frame), Some(&raw mut bytes_written), None) }
            .map_err(|error| InputServiceError::Io(error.to_string()))?;

        if usize::try_from(bytes_written).unwrap_or(0) != frame.len() {
            return Err(InputServiceError::Io(format!(
                "partial helper pipe write: {bytes_written} of {} bytes",
                frame.len()
            )));
        }
        Ok(())
    }

    #[derive(Debug)]
    pub(crate) struct HelperClient {
        pipe: OwnedHandle,
        decoder: FrameDecoder,
        server_pid: u32,
        ui_access: bool,
    }

    impl HelperClient {
        fn connect_once() -> Result<Self, InputServiceError> {
            let pipe = open_client_pipe_once()?;
            Self::handshake(pipe)
        }

        fn connect_with_timeout(timeout: Duration) -> Result<Self, InputServiceError> {
            let deadline = Instant::now() + timeout;

            loop {
                let error = match Self::connect_once() {
                    Ok(client) => return Ok(client),
                    Err(
                        error @ (InputServiceError::PeerVerification(_)
                        | InputServiceError::Protocol(_)),
                    ) => return Err(error),
                    Err(error) => error,
                };

                if Instant::now() >= deadline {
                    return Err(error);
                }
                thread::sleep(CONNECT_RETRY_INTERVAL);
            }
        }

        fn handshake(pipe: OwnedHandle) -> Result<Self, InputServiceError> {
            let mut peer_pid = 0_u32;
            // SAFETY: output points to a valid local u32 and pipe is a client pipe handle.
            unsafe { GetNamedPipeServerProcessId(pipe.get(), &raw mut peer_pid) }
                .map_err(|error| InputServiceError::Io(error.to_string()))?;

            let own = own_executable_path()?;
            let peer_image = process_image_path(peer_pid);
            if !peer_path_pinned(Some(&own), peer_image.as_deref(), HELPER_FILE_NAME) {
                return Err(InputServiceError::PeerVerification(format!(
                    "pipe server pid={peer_pid} is not the pinned helper executable"
                )));
            }

            let mut decoder = FrameDecoder::default();
            write_frame(
                pipe.get(),
                Message::Handshake {
                    client_pid: current_process_id(),
                },
            )?;
            let response = read_frame(pipe.get(), &mut decoder)?;
            let Message::HandshakeAccepted {
                server_pid,
                ui_access,
            } = response
            else {
                return Err(InputServiceError::Rejected(RejectReason::UnknownCommand));
            };

            if server_pid != peer_pid {
                return Err(InputServiceError::PeerVerification(format!(
                    "handshake pid {server_pid} does not match pipe server pid {peer_pid}"
                )));
            }

            Ok(Self {
                pipe,
                decoder,
                server_pid,
                ui_access,
            })
        }

        /// Returns the helper's reported `UIAccess` state after a live request/response probe.
        pub(crate) fn ui_access(&mut self) -> bool {
            self.request(Message::Ping {
                nonce: LIVENESS_NONCE,
            })
            .is_ok()
                && self.ui_access
        }

        #[must_use]
        pub(crate) const fn server_pid(&self) -> u32 {
            self.server_pid
        }

        pub(crate) fn request(&mut self, request: Message) -> Result<(), InputServiceError> {
            write_frame(self.pipe.get(), request)?;
            match read_frame(self.pipe.get(), &mut self.decoder)? {
                Message::Ack {
                    accepted: true,
                    detail: RejectReason::None,
                } => Ok(()),
                Message::Ack {
                    accepted: false,
                    detail,
                } => Err(InputServiceError::Rejected(detail)),
                _ => Err(InputServiceError::Rejected(RejectReason::InvalidPayload)),
            }
        }

        pub(crate) fn shutdown(&mut self) -> Result<(), InputServiceError> {
            self.request(Message::Shutdown)
        }
    }

    pub(crate) fn connect_or_spawn_helper() -> Result<HelperClient, InputServiceError> {
        if let Ok(client) = HelperClient::connect_once() {
            return Ok(client);
        }

        let helper = helper_executable_path()?;
        if !helper.is_file() {
            return Err(InputServiceError::HelperMissing);
        }

        spawn_helper_process()?;
        HelperClient::connect_with_timeout(CONNECT_TOTAL_TIMEOUT)
    }

    fn create_server_pipe() -> Result<OwnedHandle, InputServiceError> {
        let name = wide(&helper_pipe_name()?);
        // SAFETY: the name is a valid NUL-terminated UTF-16 string; the pipe is local-only and
        // synchronous. Default token DACL remains the OS security boundary for the local object.
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                PIPE_BUFFER_SIZE,
                PIPE_BUFFER_SIZE,
                0,
                None,
            )
        };
        if handle.is_invalid() {
            return Err(InputServiceError::PipeCreate(
                WindowsError::from_thread().to_string(),
            ));
        }
        Ok(OwnedHandle(handle))
    }

    fn wait_for_client(pipe: HANDLE) -> Result<(), InputServiceError> {
        // SAFETY: pipe is a server instance returned by CreateNamedPipeW.
        match unsafe { ConnectNamedPipe(pipe, None) } {
            Ok(()) => Ok(()),
            Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0) => Ok(()),
            Err(error) => Err(InputServiceError::Connect(error.to_string())),
        }
    }

    fn verify_client(pipe: HANDLE) -> Result<u32, InputServiceError> {
        let mut client_pid = 0_u32;
        // SAFETY: output points to a valid local u32 and pipe is the server handle.
        unsafe { GetNamedPipeClientProcessId(pipe, &raw mut client_pid) }
            .map_err(|error| InputServiceError::PeerVerification(error.to_string()))?;

        let own = own_executable_path()?;
        let client_image = process_image_path(client_pid);
        if !peer_path_pinned(Some(&own), client_image.as_deref(), APPLICATION_FILE_NAME) {
            return Err(InputServiceError::PeerVerification(format!(
                "pipe client pid={client_pid} is not the pinned NumFlow executable"
            )));
        }
        Ok(client_pid)
    }

    fn ack_for_pointer_result(result: &Result<(), crate::PointerError>) -> Message {
        match result {
            Ok(()) => Message::Ack {
                accepted: true,
                detail: RejectReason::None,
            },
            Err(_) => Message::Ack {
                accepted: false,
                detail: RejectReason::InjectionFailed,
            },
        }
    }

    fn serve_connection(pipe: HANDLE, expected_client_pid: u32) -> Result<(), InputServiceError> {
        let mut decoder = FrameDecoder::default();
        let first = read_frame(pipe, &mut decoder)?;
        let Message::Handshake { client_pid } = first else {
            return Err(InputServiceError::Rejected(RejectReason::NotReady));
        };
        if client_pid != expected_client_pid {
            return Err(InputServiceError::PeerVerification(format!(
                "handshake pid {client_pid} does not match pipe client pid {expected_client_pid}"
            )));
        }

        write_frame(
            pipe,
            Message::HandshakeAccepted {
                server_pid: current_process_id(),
                ui_access: current_process_ui_access().unwrap_or(false),
            },
        )?;

        let mut pointer = DirectWindowsPointer::default();
        loop {
            let request = match read_frame(pipe, &mut decoder) {
                Ok(request) => request,
                Err(error) => {
                    let _ = pointer.release_all();
                    return Err(error);
                }
            };

            let (response, shutdown) = match request {
                Message::Ping { .. } => (
                    Message::Ack {
                        accepted: true,
                        detail: RejectReason::None,
                    },
                    false,
                ),
                Message::PointerMove { dx, dy } => (
                    ack_for_pointer_result(&pointer.move_relative(dx, dy)),
                    false,
                ),
                Message::PointerButton { button, action } => {
                    let result = match action {
                        ButtonAction::Down => pointer.button_down(button),
                        ButtonAction::Up => pointer.button_up(button),
                    };
                    (ack_for_pointer_result(&result), false)
                }
                Message::Click { button } => {
                    (ack_for_pointer_result(&pointer.click(button)), false)
                }
                Message::DoubleClick { button } => {
                    (ack_for_pointer_result(&pointer.double_click(button)), false)
                }
                Message::ReleaseAll => (ack_for_pointer_result(&pointer.release_all()), false),
                Message::Shutdown => (ack_for_pointer_result(&pointer.release_all()), true),
                Message::Handshake { .. }
                | Message::HandshakeAccepted { .. }
                | Message::Ack { .. } => (
                    Message::Ack {
                        accepted: false,
                        detail: RejectReason::UnknownCommand,
                    },
                    false,
                ),
            };

            if let Err(error) = write_frame(pipe, response) {
                let _ = pointer.release_all();
                return Err(error);
            }
            if shutdown {
                return Ok(());
            }
        }
    }

    /// Runs the one-owner `UIAccess` helper service until the owner disconnects or requests shutdown.
    ///
    /// # Errors
    ///
    /// Returns [`InputServiceError`] when the helper cannot create or connect its pipe, verify the
    /// owning process, decode the restricted protocol, or complete the service loop safely.
    pub fn run_input_helper() -> Result<(), InputServiceError> {
        let pipe = create_server_pipe()?;
        wait_for_client(pipe.get())?;
        let client_pid = verify_client(pipe.get())?;
        let result = serve_connection(pipe.get(), client_pid);
        // SAFETY: the handle is a named-pipe server instance owned by this process.
        unsafe {
            let _ = DisconnectNamedPipe(pipe.get());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        time::{Duration, Instant},
    };

    use super::{peer_path_pinned, pipe_name_for_session, reconnect_allowed};

    #[test]
    fn pipe_name_is_scoped_to_the_windows_session() {
        assert_eq!(pipe_name_for_session(42), r"\\.\pipe\numflow-input-42");
    }

    #[test]
    fn peer_pinning_requires_expected_name_and_same_directory() {
        let own = Path::new(r"C:\Program Files\NumFlow\numflow.exe");
        assert!(peer_path_pinned(
            Some(own),
            Some(r"c:\program files\numflow\NUMFLOW-INPUT.EXE"),
            "numflow-input.exe"
        ));
        assert!(!peer_path_pinned(
            Some(own),
            Some(r"C:\Users\Public\numflow-input.exe"),
            "numflow-input.exe"
        ));
        assert!(!peer_path_pinned(
            Some(own),
            Some(r"C:\Program Files\NumFlow\other.exe"),
            "numflow-input.exe"
        ));
    }

    #[test]
    fn reconnect_cooldown_blocks_tight_retry_loops() {
        let now = Instant::now();
        assert!(reconnect_allowed(None, now, Duration::from_secs(2)));
        assert!(!reconnect_allowed(
            Some(now),
            now + Duration::from_millis(500),
            Duration::from_secs(2)
        ));
        assert!(reconnect_allowed(
            Some(now),
            now + Duration::from_secs(2),
            Duration::from_secs(2)
        ));
    }
}
