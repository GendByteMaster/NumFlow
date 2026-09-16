//! Restricted named-pipe transport and ownership handshake for the `numflow-input` helper.
//!
//! The transport carries only the typed messages from [crate::input_protocol]; every other frame is
//! rejected. Ownership is verified in both directions before the first pointer command is accepted:
//! each peer resolves the other side's process id from the pipe, opens that process with query-only
//! access, and requires its image to be the expected NumFlow executable in the same directory as
//! its own executable. Per Microsoft documentation the pipe DACL is the actual security boundary;
//! peer pinning is a hygiene measure that keeps the pipe private to the NumFlow installation.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use thiserror::Error;

use crate::input_protocol::{ProtocolError, RejectReason};

/// File name of the helper executable, always installed next to `numflow.exe`.
pub const HELPER_FILE_NAME: &str = "numflow-input.exe";
/// File name of the main application executable that owns the helper.
pub const APPLICATION_FILE_NAME: &str = "numflow.exe";
/// How long the application waits before retrying a helper spawn or reconnect.
pub const RECONNECT_COOLDOWN: Duration = Duration::from_millis(2_000);

/// Failure modes of the helper transport and service.
#[derive(Debug, Error)]
pub enum InputServiceError {
    /// The helper pipe could not be created.
    #[error("failed to create the numflow-input named pipe: {0}")]
    PipeCreate(String),
    /// The application could not reach the helper pipe.
    #[error("failed to connect to the numflow-input helper pipe: {0}")]
    Connect(String),
    /// A pipe peer failed the same-directory executable pinning check.
    #[error("numflow-input peer verification failed: {0}")]
    PeerVerification(String),
    /// The helper executable is not installed next to the main application.
    #[error("numflow-input helper is not installed next to NumFlow")]
    HelperMissing,
    /// The helper process could not be started.
    #[error("failed to start the numflow-input helper: {0}")]
    Spawn(String),
    /// Pipe I/O failed.
    #[error("numflow-input helper pipe I/O failed: {0}")]
    Io(String),
    /// The pipe carried a frame that violates the protocol whitelist.
    #[error("numflow-input helper protocol violation: {0}")]
    Protocol(#[from] ProtocolError),
    /// The helper refused or could not complete a request.
    #[error("the numflow-input helper rejected the request: {}", .0.code())]
    Rejected(#[from] RejectReason),
}

/// Returns the per-session helper pipe name for a Windows session id.
#[must_use]
pub fn pipe_name_for_session(session_id: u32) -> String {
    format!(r"\\.\pipe\numflow-input-{session_id}")
}

/// Decides whether a resolved peer executable is pinned to the NumFlow installation.
///
/// The peer must exist, and both the file name and the containing directory must match the own
/// executable's directory exactly (case-insensitively, because Windows paths are
/// case-insensitive). In production both peers are pinned to the same install directory, which
/// only administrators can write to.
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
    let peer_directory = peer_path.parent().map_or(String::new(), normalize_path_text);
    let peer_file = peer_path
        .file_name()
        .map_or(String::new(), |name| name.to_string_lossy().to_lowercase());

    own.parent().is_some()
        && peer_path.file_name().is_some()
        && peer_file == expected_peer_file.to_lowercase()
        && peer_directory == own_directory
}

/// Decides whether a helper reconnect attempt may start at `now`.
///
/// Cooldowns keep a failing helper from turning every motion tick into a spawn attempt.
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
pub(crate) mod windows_impl {
    //! Win32 named-pipe client, server, and handshake implementation.

    use std::{
        os::windows::process::CommandExt,
        path::PathBuf,
        process::Command,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{
                CloseHandle, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
                INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
            },
            Storage::FileSystem::{
                CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAGS_AND_ATTRIBUTES,
                FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
            },
            System::{
                Pipes::{
                    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
                    GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, PeekNamedPipe,
                    NAMED_PIPE_MODE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
                },
                RemoteDesktop::ProcessIdToSessionId,
                Threading::{
                    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW,
                    WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                    PROCESS_SYNCHRONIZE,
                },
            },
        },
    };

    use crate::{
        diagnostics::current_process_ui_access,
        input_protocol::{ButtonAction, FrameDecoder, Message, RejectReason, HEADER_LEN},
        pointer::WindowsPointer,
    };

    use super::{
        InputServiceError, APPLICATION_FILE_NAME, HELPER_FILE_NAME, RECONNECT_COOLDOWN,
        peer_path_pinned, pipe_name_for_session,
    };

    /// `CREATE_NO_WINDOW` for the helper process spawn.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    /// How often the served connection polls for owner death and incoming frames.
    const SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(75);
    /// How long the application waits for the helper pipe to appear after spawning it.
    const CONNECT_TOTAL_TIMEOUT: Duration = Duration::from_secs(2);
    const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(40);

    /// Returns this process's Windows session id.
    fn current_session_id() -> Result<u32, InputServiceError> {
        let mut session_id = 0_u32;
        // SAFETY: the pointer targets a local u32 output; the call only reads the own process id.
        unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut session_id) }
            .map_err(|error| InputServiceError::Io(error.to_string()))?;
        Ok(session_id)
    }

    /// Returns the per-session helper pipe name for this process's session.
    pub(super) fn helper_pipe_name() -> Result<String, InputServiceError> {
        Ok(pipe_name_for_session(current_session_id()?))
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn own_executable_path() -> Result<PathBuf, InputServiceError> {
        std::env::current_exe().map_err(|error| InputServiceError::Io(error.to_string()))
    }

    /// Resolves the full image path of a process by id, for peer pinning.
    fn process_image_path(process_id: u32) -> Option<String> {
        // SAFETY: OpenProcess only requests query access; the output buffer and its size pointer
        // target locals of this function and the handle is closed before returning.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;
            let mut buffer = [0_u16; 1024];
            let mut size =
                u32::try_from(buffer.len()).expect("image name buffer size fits in u32");
            let result = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &raw mut size,
            );
            let _ = CloseHandle(process);
            result.ok()?;
            let len = usize::try_from(size).unwrap_or(0);
            Some(String::from_utf16_lossy(&buffer[..len.min(buffer.len())]))
        }
    }

    /// Spawns the helper executable installed next to the main application.
    pub(super) fn spawn_helper_process() -> Result<u32, InputServiceError> {
        let helper = own_executable_path()?
            .parent()
            .ok_or(InputServiceError::HelperMissing)?
            .join(HELPER_FILE_NAME);
        if !helper.is_file() {
            return Err(InputServiceError::HelperMissing);
        }

        // `CREATE_NO_WINDOW` keeps the helper console-free in the interactive session.
        let child = Command::new(&helper)
            .arg("--input-service")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| InputServiceError::Spawn(error.to_string()))?;
        Ok(child.id())
    }

    /// Application-side connection to the `numflow-input` helper.
    ///
    /// The connection is request/response: every request waits for the helper's typed
    /// acknowledgement. The helper's process identity is verified twice — from the pipe peer id and
    /// from the handshake payload — before the first request is accepted.
    pub(super) struct HelperClient {
        pipe: HANDLE,
        decoder: FrameDecoder,
        server_pid: u32,
        ui_access: bool,
    }

    impl HelperClient {
        /// Connects to the helper owned by this session, pinning the peer to `HELPER_FILE_NAME`.
        pub(super) fn connect() -> Result<Self, InputServiceError> {
            let own = own_executable_path()?;
            Self::connect_with_peer_expectation(&own, HELPER_FILE_NAME)
        }

        /// Connects with an explicit peer expectation; tests pin both peers to the same
        /// in-process test executable with this entry point.
        pub(super) fn connect_with_peer_expectation(
            own_executable: &std::path::Path,
            expected_peer_file: &str,
        ) -> Result<Self, InputServiceError> {
            let name = wide(&helper_pipe_name()?);
            let deadline = Instant::now() + CONNECT_TOTAL_TIMEOUT;
            let mut pipe: Result<HANDLE, InputServiceError> = Err(InputServiceError::Connect(
                "helper pipe is not available yet".to_owned(),
            ));

            while Instant::now() < deadline {
                // SAFETY: the pipe name is a NUL-terminated wide string owned for the duration of
                // the call; no security attributes are inherited.
                match unsafe {
                    CreateFileW(
                        windows::core::PCWSTR(name.as_ptr()),
                        GENERIC_READ.0 | GENERIC_WRITE.0,
                        FILE_SHARE_NONE,
                        None,
                        OPEN_EXISTING,
                        FILE_FLAGS_AND_ATTRIBUTES(0),
                        None,
                    )
                } {
                    Ok(handle) => {
                        pipe = Ok(handle);
                        break;
                    }
                    Err(error) => {
                        pipe = Err(InputServiceError::Connect(error.to_string()));
                    }
                }

                thread::sleep(CONNECT_RETRY_INTERVAL);
            }

            let pipe = pipe?;
            if pipe.is_invalid() {
                return Err(InputServiceError::Connect(
                    "CreateFileW returned an invalid pipe handle".to_owned(),
                ));
            }

            match Self::handshake(pipe, own_executable, expected_peer_file) {
                Ok(client) => Ok(client),
                Err(error) => {
                    // SAFETY: the pipe handle was created above and is not shared.
                    unsafe {
                        let _ = CloseHandle(pipe);
                    }
                    Err(error)
                }
            }
        }

        fn handshake(
            pipe: HANDLE,
            own_executable: &std::path::Path,
            expected_peer_file: &str,
        ) -> Result<Self, InputServiceError> {
            // SAFETY: the handle is a valid pipe created above; the output targets a local u32.
            let peer_pid = unsafe {
                let mut pid = 0_u32;
                GetNamedPipeServerProcessId(pipe, &raw mut pid)
                    .map_err(|error| InputServiceError::Io(error.to_string()))?;
                pid
            };

            let peer_image = process_image_path(peer_pid);
            if !peer_path_pinned(
                Some(own_executable),
                peer_image.as_deref(),
                expected_peer_file,
            ) {
                return Err(InputServiceError::PeerVerification(format!(
                    "helper pipe peer pid={peer_pid} is not the pinned helper executable"
                )));
            }

            let mut client = Self {
                pipe,
                decoder: FrameDecoder::default(),
                server_pid: peer_pid,
                ui_access: false,
            };

            client.send_frame(&Message::Handshake {
                client_pid: GetCurrentProcessId(),
            })?;
            match client.read_frame()? {
                Message::HandshakeAccepted {
                    server_pid,
                    ui_access,
                } => {
                    if server_pid != peer_pid {
                        return Err(InputServiceError::PeerVerification(format!(
                            "handshake pid {server_pid} does not match the pipe peer {peer_pid}"
                        )));
                    }
                    client.server_pid = server_pid;
                    client.ui_access = ui_access;
                    Ok(client)
                }
                _ => Err(InputServiceError::Rejected(RejectReason::UnknownCommand)),
            }
        }

        /// The helper's actual UIAccess token state, as reported by the handshake.
        #[must_use]
        pub(super) const fn ui_access(&self) -> bool {
            self.ui_access
        }

        /// The verified helper process id.
        #[must_use]
        pub(super) const fn server_pid(&self) -> u32 {
            self.server_pid
        }

        /// Sends a request and waits for its acknowledgement.
        pub(super) fn request(&mut self, request: Message) -> Result<(), InputServiceError> {
            self.send_frame(&request)?;
            match self.read_frame()? {
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

        /// Asks the helper to release injected state and exit; transport failures are ignored so
        /// shutdown paths never block on a dead helper.
        pub(super) fn request_shutdown(&mut self) {
            if self.send_frame(&Message::Shutdown).is_ok() {
                let _ = self.read_frame();
            }
        }

        fn send_frame(&mut self, message: &Message) -> Result<(), InputServiceError> {
            let frame = message.encode();
            // SAFETY: the pipe handle is valid; the buffer lives for the duration of the call.
            unsafe {
                WriteFile(self.pipe, Some(&frame), None, None)
                    .map_err(|error| InputServiceError::Io(error.to_string()))
            }
        }

        fn read_frame(&mut self) -> Result<Message, InputServiceError> {
            let mut chunk = [0_u8; HEADER_LEN + MAX_PAYLOAD_LEN];
            loop {
                if let Some(message) = self.decoder.next_frame()? {
                    return Ok(message);
                }

                // SAFETY: the pipe handle is valid; the buffer outlives the call.
                let read = unsafe {
                    let mut bytes_read = 0_u32;
                    ReadFile(self.pipe, Some(&mut chunk), Some(&raw mut bytes_read), None)
                        .map_err(|error| InputServiceError::Io(error.to_string()))?;
                    usize::try_from(bytes_read).unwrap_or(0)
                };

                if read == 0 {
                    return Err(InputServiceError::Io(
                        "helper closed the pipe connection".to_owned(),
                    ));
                }
                self.decoder.push(&chunk[..read]);
            }
        }
    }

    impl Drop for HelperClient {
        fn drop(&mut self) {
            self.request_shutdown();
            // SAFETY: the pipe handle was created by this client and is dropped exactly once.
            unsafe {
                let _ = CloseHandle(self.pipe);
            }
        }
    }


}

