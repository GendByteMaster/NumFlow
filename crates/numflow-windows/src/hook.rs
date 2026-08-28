use std::{
    ffi::c_void,
    io,
    mem::size_of,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use windows::{
    Win32::{
        Devices::HumanInterfaceDevice::GUID_DEVINTERFACE_KEYBOARD,
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, ERROR_INVALID_HOOK_HANDLE, HANDLE, HINSTANCE, HWND, LPARAM,
            LRESULT, WIN32_ERROR, WPARAM,
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            Power::{
                DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
                RegisterSuspendResumeNotification, UnregisterSuspendResumeNotification,
            },
            RemoteDesktop::{
                NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
                WTSUnRegisterSessionNotification,
            },
            Threading::{GetCurrentProcessId, GetCurrentThreadId},
        },
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
            Input::{
                GetRawInputDeviceList,
                KeyboardAndMouse::{
                    GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
                    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, VK_NUMLOCK,
                },
                RAWINPUTDEVICE, RAWINPUTDEVICELIST, RIDEV_REMOVE, RIM_TYPEKEYBOARD,
                RegisterRawInputDevices,
            },
            WindowsAndMessaging::{
                CallNextHookEx, CreateWindowExW, DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE,
                DBT_DEVTYP_DEVICEINTERFACE, DEV_BROADCAST_DEVICEINTERFACE_W,
                DEVICE_NOTIFY_CALLBACK, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DestroyWindow,
                DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GIDC_ARRIVAL, GIDC_REMOVAL, GetMessageW,
                HDEVNOTIFY, HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED,
                MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL, PBT_APMRESUMESUSPEND,
                PBT_APMSUSPEND, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, RegisterClassW,
                RegisterDeviceNotificationW, SetWindowsHookExW, UnhookWindowsHookEx,
                UnregisterDeviceNotification, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE,
                WINEVENT_OUTOFCONTEXT, WM_APP, WM_DEVICECHANGE, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
                WM_SYSKEYDOWN, WM_SYSKEYUP, WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_SESSION_LOCK,
                WTS_SESSION_UNLOCK,
            },
        },
    },
    core::{Error as WindowsError, PCWSTR},
};

use crate::{KeyState, PhysicalKeyEvent, map_numpad_key};

const DEFAULT_QUEUE_CAPACITY: usize = 128;
const MIN_LIFECYCLE_QUEUE_CAPACITY: usize = 2;
const NUMFLOW_NUM_LOCK_INJECTION_TAG: usize = 0x4E46_4E4C;
const NUM_LOCK_SCAN_CODE: u16 = 0x45;
const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
const HID_USAGE_GENERIC_KEYBOARD: u16 = 0x06;
const WM_NUMFLOW_SUSPEND: u32 = WM_APP + 0x4E1;
const WM_NUMFLOW_RESUME_AUTOMATIC: u32 = WM_APP + 0x4E2;
const WM_NUMFLOW_RESUME_USER: u32 = WM_APP + 0x4E3;
const WM_NUMFLOW_SESSION_UNLOCK: u32 = WM_APP + 0x4E4;
const WM_NUMFLOW_DESKTOP_READY: u32 = WM_APP + 0x4E5;
const WM_NUMFLOW_FOREGROUND_CHANGED: u32 = WM_APP + 0x4E6;
const WM_NUMFLOW_KEYBOARD_DEVICE_CHANGED: u32 = WM_APP + 0x4E7;
const WM_NUMFLOW_INPUT_RESYNC: u32 = WM_APP + 0x4E8;
const WTS_SESSION_DESKTOP_READY_REASON: u32 = 0x0F;
const RESUME_STAGE_IDLE: u32 = 0;
const RESUME_STAGE_AUTOMATIC: u32 = 1;
const RESUME_STAGE_USER: u32 = 2;
const SESSION_STAGE_IDLE: u32 = 0;
const SESSION_STAGE_UNLOCK: u32 = 1;
const SESSION_STAGE_DESKTOP_READY: u32 = 2;
const INPUT_RUNTIME_RUNNING: u32 = 0;
const INPUT_RUNTIME_SUSPENDED: u32 = 1;
const INPUT_RUNTIME_RECOVERING: u32 = 2;

static POWER_NOTIFICATION_ORDER: Mutex<()> = Mutex::new(());
static NUM_LOCK_STATE_ORDER: Mutex<()> = Mutex::new(());
static POWER_RESUME_STAGE: AtomicU32 = AtomicU32::new(RESUME_STAGE_IDLE);
static SESSION_RESUME_STAGE: AtomicU32 = AtomicU32::new(SESSION_STAGE_IDLE);
static EVENT_DISPATCHER: OnceLock<Mutex<Option<HookDispatcher>>> = OnceLock::new();
static INTERCEPTION_ENABLED: AtomicBool = AtomicBool::new(false);
static NUM_LOCK_ON: AtomicBool = AtomicBool::new(true);
static NUM_LOCK_KEY_DOWN: AtomicBool = AtomicBool::new(false);
static RESUME_NUM_LOCK_GUARD: AtomicBool = AtomicBool::new(false);
static SESSION_LOCK_PENDING: AtomicBool = AtomicBool::new(false);
static RESUME_WINDOWS_NUM_LOCK_MISMATCH: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static HOOK_GENERATION: AtomicU32 = AtomicU32::new(0);
static INPUT_RUNTIME_STATE: AtomicU32 = AtomicU32::new(INPUT_RUNTIME_RECOVERING);
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static KEYBOARD_DEVICE_NOTIFICATIONS_REGISTERED: AtomicBool = AtomicBool::new(false);
static WINIT_RAW_KEYBOARD_REGISTRATION_DISABLED: AtomicBool = AtomicBool::new(false);
static HOOK_EVENT_COUNT: AtomicU64 = AtomicU64::new(0);
static HOOK_NUMPAD_EVENT_COUNT: AtomicU64 = AtomicU64::new(0);
static HOOK_NUMPAD_DISPATCHED_COUNT: AtomicU64 = AtomicU64::new(0);
static HOOK_NUMPAD_DROPPED_COUNT: AtomicU64 = AtomicU64::new(0);
static RUNTIME_NUMPAD_EVENT_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardHookEvent {
    Key(PhysicalKeyEvent),
    NumLockChanged {
        num_lock_on: bool,
        sync_system: bool,
        play_feedback: bool,
    },
    InputUnavailable {
        reason: InputResyncReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRuntimeState {
    Running,
    Suspended,
    Recovering,
}

impl InputRuntimeState {
    fn from_raw(value: u32) -> Self {
        match value {
            INPUT_RUNTIME_RUNNING => Self::Running,
            INPUT_RUNTIME_SUSPENDED => Self::Suspended,
            _ => Self::Recovering,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputResyncReason {
    Startup,
    ResumeAutomatic,
    ResumeUser,
    SessionUnlock,
    DesktopReady,
    ForegroundChanged,
    KeyboardDeviceChanged,
    HookFailure,
    NumLockChanged,
}

impl InputResyncReason {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::ResumeAutomatic => "resume-automatic",
            Self::ResumeUser => "resume-user",
            Self::SessionUnlock => "session-unlock",
            Self::DesktopReady => "desktop-ready",
            Self::ForegroundChanged => "foreground-changed",
            Self::KeyboardDeviceChanged => "keyboard-device-changed",
            Self::HookFailure => "hook-failure",
            Self::NumLockChanged => "numlock-changed",
        }
    }

    const fn code(self) -> usize {
        match self {
            Self::Startup => 1,
            Self::ResumeAutomatic => 2,
            Self::ResumeUser => 3,
            Self::SessionUnlock => 4,
            Self::DesktopReady => 5,
            Self::ForegroundChanged => 6,
            Self::KeyboardDeviceChanged => 7,
            Self::HookFailure => 8,
            Self::NumLockChanged => 9,
        }
    }

    const fn from_code(code: usize) -> Option<Self> {
        Some(match code {
            1 => Self::Startup,
            2 => Self::ResumeAutomatic,
            3 => Self::ResumeUser,
            4 => Self::SessionUnlock,
            5 => Self::DesktopReady,
            6 => Self::ForegroundChanged,
            7 => Self::KeyboardDeviceChanged,
            8 => Self::HookFailure,
            9 => Self::NumLockChanged,
            _ => return None,
        })
    }
}

#[derive(Debug)]
struct HookDispatcher {
    sender: Sender<KeyboardHookEvent>,
    overflow_reader: Receiver<KeyboardHookEvent>,
}

#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("the NumFlow keyboard hook is already active")]
    AlreadyActive,
    #[error("failed to spawn the Windows hook thread: {0}")]
    ThreadSpawn(#[source] io::Error),
    #[error("failed to install WH_KEYBOARD_LL: {0}")]
    Install(#[source] WindowsError),
    #[error("failed to register Windows suspend/resume notifications: {0}")]
    PowerNotification(#[source] WindowsError),
    #[error("the Windows hook message loop failed: {0}")]
    MessageLoop(#[source] WindowsError),
    #[error("failed to stop the Windows hook thread: {0}")]
    Stop(#[source] WindowsError),
    #[error("the Windows hook thread terminated unexpectedly")]
    ThreadTerminated,
    #[error("the Windows hook thread panicked")]
    ThreadPanicked,
}

fn keyboard_device_notification_filter() -> DEV_BROADCAST_DEVICEINTERFACE_W {
    DEV_BROADCAST_DEVICEINTERFACE_W {
        dbcc_size: u32::try_from(size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>())
            .expect("device-notification filter size must fit in a Win32 DWORD"),
        dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
        dbcc_classguid: GUID_DEVINTERFACE_KEYBOARD,
        ..DEV_BROADCAST_DEVICEINTERFACE_W::default()
    }
}

fn raw_keyboard_removal_device() -> RAWINPUTDEVICE {
    RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_GENERIC,
        usUsage: HID_USAGE_GENERIC_KEYBOARD,
        dwFlags: RIDEV_REMOVE,
        ..RAWINPUTDEVICE::default()
    }
}

/// Removes winit's process-wide raw-keyboard registration after Slint creates its event loop.
///
/// `NumFlow` receives global `NumPad` input exclusively through `WH_KEYBOARD_LL`. Keeping winit's
/// keyboard Raw Input registration makes delivery of that low-level hook foreground-dependent
/// when a `NumFlow` window owns focus. Normal Slint text/key input continues through window messages;
/// mouse Raw Input registration is not changed. Keyboard hotplug is observed independently through
/// `RegisterDeviceNotificationW`, so this registration is never recreated by recovery.
///
/// The operation is process-wide and idempotent.
///
/// # Errors
///
/// Returns the Win32 error when the keyboard Raw Input registration cannot be removed.
///
/// # Panics
///
/// Panics only if the compile-time `RAWINPUTDEVICE` size cannot fit in a Win32 `UINT`.
pub fn disable_winit_raw_keyboard_registration() -> Result<(), WindowsError> {
    if WINIT_RAW_KEYBOARD_REGISTRATION_DISABLED.load(Ordering::Acquire) {
        return Ok(());
    }

    let device = raw_keyboard_removal_device();
    let device_size = u32::try_from(size_of::<RAWINPUTDEVICE>())
        .expect("RAWINPUTDEVICE size must fit in a Win32 UINT");
    unsafe { RegisterRawInputDevices(&[device], device_size) }?;
    WINIT_RAW_KEYBOARD_REGISTRATION_DISABLED.store(true, Ordering::Release);
    eprintln!(
        "NumFlow: winit raw keyboard registration disabled; WH_KEYBOARD_LL owns global NumPad input"
    );
    Ok(())
}

#[derive(Debug)]
pub struct KeyboardHook {
    thread_id: u32,
    join: Option<JoinHandle<Result<(), HookError>>>,
}

impl KeyboardHook {
    /// Starts the global low-level keyboard hook with the default event queue capacity.
    ///
    /// # Errors
    ///
    /// Returns [`HookError`] if the hook thread cannot be spawned, the Win32 hook cannot be
    /// installed, another `NumFlow` hook is already active, power notifications cannot be
    /// registered, or the hook thread exits before setup completes.
    pub fn start() -> Result<(Self, Receiver<KeyboardHookEvent>), HookError> {
        Self::start_with_capacity(DEFAULT_QUEUE_CAPACITY)
    }

    /// Starts the global low-level keyboard hook with a bounded event queue.
    ///
    /// The queue has a minimum capacity of two. Resume recovery deliberately queues a transient
    /// cleanup state followed by the authoritative Num Lock mode, and both events must remain
    /// ordered without blocking the low-level hook callback.
    ///
    /// # Errors
    ///
    /// Returns [`HookError`] if the hook thread cannot be spawned, the Win32 hook cannot be
    /// installed, another `NumFlow` hook is already active, power notifications cannot be
    /// registered, or the hook thread exits before setup completes.
    pub fn start_with_capacity(
        queue_capacity: usize,
    ) -> Result<(Self, Receiver<KeyboardHookEvent>), HookError> {
        let capacity = queue_capacity.max(MIN_LIFECYCLE_QUEUE_CAPACITY);
        let (event_sender, event_receiver) = crossbeam_channel::bounded(capacity);
        let event_overflow_reader = event_receiver.clone();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);

        let join = thread::Builder::new()
            .name("numflow-keyboard-hook".to_owned())
            .spawn(move || hook_thread(event_sender, event_overflow_reader, &ready_sender))
            .map_err(HookError::ThreadSpawn)?;

        let thread_id = match ready_receiver.recv() {
            Ok(Ok(thread_id)) => thread_id,
            Ok(Err(error)) => {
                let _ = join.join();
                return Err(error);
            }
            Err(_) => {
                let _ = join.join();
                return Err(HookError::ThreadTerminated);
            }
        };

        INTERCEPTION_ENABLED.store(false, Ordering::Release);

        Ok((
            Self {
                thread_id,
                join: Some(join),
            },
            event_receiver,
        ))
    }

    #[must_use]
    pub fn interception_enabled(&self) -> bool {
        INTERCEPTION_ENABLED.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn num_lock_on(&self) -> bool {
        NUM_LOCK_ON.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn input_runtime_state(&self) -> InputRuntimeState {
        InputRuntimeState::from_raw(INPUT_RUNTIME_STATE.load(Ordering::Acquire))
    }

    #[must_use]
    pub fn hook_alive(&self) -> bool {
        HOOK_INSTALLED.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn hook_event_count(&self) -> u64 {
        HOOK_EVENT_COUNT.load(Ordering::Acquire)
    }

    pub fn record_runtime_numpad_event(&self) {
        RUNTIME_NUMPAD_EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    /// Asks the hook thread to verify its input lifecycle state and low-level keyboard hook.
    ///
    /// The request is posted to the existing message loop; it never installs a hook from the
    /// runtime worker and therefore cannot race the hook thread's retire-before-install ordering.
    #[must_use]
    pub fn resync_input_state(&self, reason: InputResyncReason) -> bool {
        unsafe {
            PostThreadMessageW(
                self.thread_id,
                WM_NUMFLOW_INPUT_RESYNC,
                WPARAM(reason.code()),
                LPARAM(0),
            )
            .is_ok()
        }
    }

    /// Synchronizes the tracked and Windows Num Lock state with an explicit runtime request.
    ///
    /// A tagged `SendInput` toggle is emitted only when the requested state differs from the
    /// tracked physical state. `NumFlow` updates interception around that toggle so enabling pointer
    /// control cannot leak an immediately-following `NumPad` key, while a failed injection restores
    /// the previous interception state.
    ///
    /// Returns `false` when Windows did not accept the complete Num Lock replay sequence.
    ///
    /// # Panics
    ///
    /// Panics only if compile-time Win32 input structure sizes cannot fit their API integer types.
    #[must_use]
    pub fn set_num_lock_on(&self, num_lock_on: bool) -> bool {
        let Ok(_state_guard) = NUM_LOCK_STATE_ORDER.lock() else {
            return false;
        };
        let current = self.num_lock_on();
        if current == num_lock_on {
            self.set_interception_enabled(!num_lock_on);
            return true;
        }

        let previous_interception = self.interception_enabled();
        INTERCEPTION_ENABLED.store(!num_lock_on, Ordering::Release);

        if !replay_num_lock_to_windows() {
            INTERCEPTION_ENABLED.store(previous_interception, Ordering::Release);
            return false;
        }

        NUM_LOCK_ON.store(num_lock_on, Ordering::Release);
        NUM_LOCK_KEY_DOWN.store(false, Ordering::Release);
        self.set_interception_enabled(!num_lock_on);
        true
    }

    pub fn set_interception_enabled(&self, enabled: bool) {
        let should_intercept = enabled && !self.num_lock_on();
        INTERCEPTION_ENABLED.store(should_intercept, Ordering::Release);
    }

    /// Replays the already-intercepted physical Num Lock toggle after the low-level hook callback
    /// has returned. Keeping input injection out of `keyboard_hook_proc` avoids re-entrant keyboard
    /// state changes while Windows is still processing the physical key-down event.
    #[must_use]
    pub fn sync_num_lock_to_windows(&self) -> bool {
        let Ok(_state_guard) = NUM_LOCK_STATE_ORDER.lock() else {
            return false;
        };
        replay_num_lock_to_windows()
    }

    pub fn emergency_disable(&self) {
        INTERCEPTION_ENABLED.store(false, Ordering::Release);
    }

    /// Disables interception, requests message-loop shutdown, and joins the hook thread.
    ///
    /// # Errors
    ///
    /// Returns [`HookError`] if posting the shutdown message fails, unhooking fails, the message
    /// loop reports a Win32 error, or the hook thread panics.
    pub fn stop(mut self) -> Result<(), HookError> {
        self.emergency_disable();
        self.request_stop()?;
        self.join_thread()
    }

    fn request_stop(&self) -> Result<(), HookError> {
        if self.join.as_ref().is_some_and(JoinHandle::is_finished) {
            return Ok(());
        }

        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0))
                .map_err(HookError::Stop)
        }
    }

    fn join_thread(&mut self) -> Result<(), HookError> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };

        match join.join() {
            Ok(result) => result,
            Err(_) => Err(HookError::ThreadPanicked),
        }
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        self.emergency_disable();
        let _ = self.request_stop();
        let _ = self.join_thread();
    }
}

fn hook_thread(
    event_sender: Sender<KeyboardHookEvent>,
    event_overflow_reader: Receiver<KeyboardHookEvent>,
    ready_sender: &SyncSender<Result<u32, HookError>>,
) -> Result<(), HookError> {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut message = MSG::default();

    // A message queue must exist before the power callback can safely PostThreadMessageW to this
    // worker.
    let _ = unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_NOREMOVE) };

    let module = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => HINSTANCE(module.0),
        Err(error) => {
            let _ = ready_sender.send(Err(HookError::Install(error)));
            return Ok(());
        }
    };

    if !register_dispatcher(event_sender, event_overflow_reader) {
        let _ = ready_sender.send(Err(HookError::AlreadyActive));
        return Ok(());
    }

    initialize_num_lock_state();

    let mut hook = match install_keyboard_hook(module) {
        Ok(hook) => Some(hook),
        Err(error) => {
            clear_dispatcher();
            let _ = ready_sender.send(Err(HookError::Install(error)));
            return Ok(());
        }
    };

    HOOK_THREAD_ID.store(thread_id, Ordering::Release);
    POWER_RESUME_STAGE.store(RESUME_STAGE_IDLE, Ordering::Release);
    SESSION_RESUME_STAGE.store(SESSION_STAGE_IDLE, Ordering::Release);
    let power_registration = match register_suspend_resume_notifications() {
        Ok(registration) => registration,
        Err(error) => {
            HOOK_THREAD_ID.store(0, Ordering::Release);
            clear_dispatcher();
            let _ = retire_keyboard_hook(&mut hook);
            let _ = ready_sender.send(Err(error));
            return Ok(());
        }
    };

    let (session_window, keyboard_device_notification) = match create_session_notification_window(
        module,
    ) {
        Ok(hwnd) => {
            eprintln!("NumFlow: session lifecycle window registered");
            let notification = match register_keyboard_device_notifications(hwnd) {
                Ok(notification) => Some(notification),
                Err(error) => {
                    eprintln!("NumFlow: keyboard device notifications unavailable: {error}");
                    None
                }
            };
            (Some(hwnd), notification)
        }
        Err(error) => {
            eprintln!(
                "NumFlow: session lifecycle notifications unavailable; using power-only recovery: {error}"
            );
            (None, None)
        }
    };

    let foreground_hook = match register_foreground_notifications() {
        Ok(hook) => {
            eprintln!("NumFlow: foreground change notifications registered");
            Some(hook)
        }
        Err(error) => {
            eprintln!("NumFlow: foreground change notifications unavailable: {error}");
            None
        }
    };

    if ready_sender.send(Ok(thread_id)).is_err() {
        HOOK_THREAD_ID.store(0, Ordering::Release);
        cleanup_keyboard_device_notifications(keyboard_device_notification);
        if let Some(hwnd) = session_window {
            cleanup_session_notification_window(hwnd);
        }
        cleanup_foreground_notifications(foreground_hook);
        let _ = unsafe { UnregisterSuspendResumeNotification(power_registration) };
        clear_dispatcher();
        let _ = retire_keyboard_hook(&mut hook);
        return Ok(());
    }

    INPUT_RUNTIME_STATE.store(INPUT_RUNTIME_RUNNING, Ordering::Release);
    let loop_result = run_message_loop(module, &mut hook, session_window);

    INTERCEPTION_ENABLED.store(false, Ordering::Release);
    HOOK_THREAD_ID.store(0, Ordering::Release);
    cleanup_keyboard_device_notifications(keyboard_device_notification);
    if let Some(hwnd) = session_window {
        cleanup_session_notification_window(hwnd);
    }
    cleanup_foreground_notifications(foreground_hook);
    let power_result = unsafe { UnregisterSuspendResumeNotification(power_registration) };
    clear_dispatcher();
    let unhook_result = retire_keyboard_hook(&mut hook);

    loop_result?;
    power_result.map_err(HookError::PowerNotification)?;
    unhook_result.map_err(HookError::Stop)
}

fn initialize_num_lock_state() {
    let key_state = unsafe { GetKeyState(i32::from(VK_NUMLOCK.0)) };
    NUM_LOCK_ON.store(key_state & 1 != 0, Ordering::Release);
    NUM_LOCK_KEY_DOWN.store(key_state < 0, Ordering::Release);
    eprintln!(
        "NumFlow: startup VK_NUMLOCK snapshot = {}; num_lock_on = {}; numflow_enabled = {}",
        key_state & 1 != 0,
        key_state & 1 != 0,
        key_state & 1 == 0
    );
}

fn install_keyboard_hook(module: HINSTANCE) -> Result<HHOOK, WindowsError> {
    let hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), Some(module), 0) }?;
    HOOK_INSTALLED.store(true, Ordering::Release);
    HOOK_GENERATION.fetch_add(1, Ordering::AcqRel);
    Ok(hook)
}

fn register_suspend_resume_notifications() -> Result<HPOWERNOTIFY, HookError> {
    let parameters = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(power_notification_callback),
        Context: std::ptr::null_mut(),
    };
    let recipient = HANDLE(std::ptr::from_ref(&parameters).cast::<c_void>().cast_mut());

    unsafe { RegisterSuspendResumeNotification(recipient, DEVICE_NOTIFY_CALLBACK) }
        .map_err(HookError::PowerNotification)
}

fn create_session_notification_window(module: HINSTANCE) -> Result<HWND, WindowsError> {
    let class_name_utf16 = "NumFlowInputLifecycleWindow"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let class_name = PCWSTR(class_name_utf16.as_ptr());
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(session_window_proc),
        hInstance: module,
        lpszClassName: class_name,
        ..WNDCLASSW::default()
    };

    let atom = unsafe { RegisterClassW(&raw const window_class) };
    if atom == 0 {
        let error = WindowsError::from_thread();
        if WIN32_ERROR::from_error(&error) != Some(ERROR_CLASS_ALREADY_EXISTS) {
            return Err(error);
        }
    }

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            class_name,
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module),
            None,
        )
    }?;

    if let Err(error) = unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }

    Ok(hwnd)
}

fn register_keyboard_device_notifications(target: HWND) -> Result<HDEVNOTIFY, WindowsError> {
    let filter = keyboard_device_notification_filter();
    let recipient = HANDLE(target.0);
    let notification = unsafe {
        RegisterDeviceNotificationW(
            recipient,
            std::ptr::from_ref(&filter).cast::<c_void>(),
            DEVICE_NOTIFY_WINDOW_HANDLE,
        )
    }?;
    KEYBOARD_DEVICE_NOTIFICATIONS_REGISTERED.store(true, Ordering::Release);
    eprintln!(
        "NumFlow: keyboard device notifications registered (source=WM_DEVICECHANGE, attached_keyboards={:?})",
        attached_raw_keyboard_count()
    );
    Ok(notification)
}

fn cleanup_keyboard_device_notifications(notification: Option<HDEVNOTIFY>) {
    if let Some(notification) = notification
        && let Err(error) = unsafe { UnregisterDeviceNotification(notification) }
    {
        eprintln!("NumFlow: failed to unregister keyboard device notifications: {error}");
    }
    KEYBOARD_DEVICE_NOTIFICATIONS_REGISTERED.store(false, Ordering::Release);
}

fn attached_raw_keyboard_count() -> Option<u32> {
    let device_size = u32::try_from(size_of::<RAWINPUTDEVICELIST>())
        .expect("RAWINPUTDEVICELIST size must fit in a Win32 UINT");
    let mut count = 0;
    let required = unsafe { GetRawInputDeviceList(None, &raw mut count, device_size) };
    if required == u32::MAX {
        return None;
    }
    let capacity = usize::try_from(count).ok()?;
    let mut devices = vec![RAWINPUTDEVICELIST::default(); capacity];
    let returned =
        unsafe { GetRawInputDeviceList(Some(devices.as_mut_ptr()), &raw mut count, device_size) };
    if returned == u32::MAX {
        return None;
    }
    devices
        .iter()
        .filter(|device| device.dwType == RIM_TYPEKEYBOARD)
        .count()
        .try_into()
        .ok()
}

fn register_foreground_notifications() -> Result<HWINEVENTHOOK, WindowsError> {
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_invalid() {
        Err(WindowsError::from_thread())
    } else {
        Ok(hook)
    }
}

fn cleanup_foreground_notifications(hook: Option<HWINEVENTHOOK>) {
    if let Some(hook) = hook
        && unsafe { !UnhookWinEvent(hook).as_bool() }
    {
        eprintln!("NumFlow: failed to unregister foreground change notifications");
    }
}

unsafe extern "system" fn foreground_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND {
        queue_foreground_notification(hwnd);
    }
}

fn queue_foreground_notification(hwnd: HWND) {
    let thread_id = HOOK_THREAD_ID.load(Ordering::Acquire);
    if thread_id == 0 {
        return;
    }
    let _ = unsafe {
        PostThreadMessageW(
            thread_id,
            WM_NUMFLOW_FOREGROUND_CHANGED,
            WPARAM(0),
            LPARAM(hwnd.0 as isize),
        )
    };
}

fn queue_keyboard_device_notification(reason: u32) {
    let thread_id = HOOK_THREAD_ID.load(Ordering::Acquire);
    if thread_id == 0 {
        return;
    }
    let _ = unsafe {
        PostThreadMessageW(
            thread_id,
            WM_NUMFLOW_KEYBOARD_DEVICE_CHANGED,
            WPARAM(reason as usize),
            LPARAM(0),
        )
    };
}

fn cleanup_session_notification_window(hwnd: HWND) {
    if let Err(error) = unsafe { WTSUnRegisterSessionNotification(hwnd) } {
        eprintln!("NumFlow: failed to unregister session lifecycle notifications: {error}");
    }
    if let Err(error) = unsafe { DestroyWindow(hwnd) } {
        eprintln!("NumFlow: failed to destroy session lifecycle window: {error}");
    }
}

unsafe extern "system" fn session_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_WTSSESSION_CHANGE {
        if let Ok(reason) = u32::try_from(wparam.0) {
            queue_session_notification(reason);
        }
        return LRESULT(0);
    }

    if message == WM_DEVICECHANGE {
        if let Ok(reason) = u32::try_from(wparam.0) {
            match reason {
                DBT_DEVICEARRIVAL => queue_keyboard_device_notification(GIDC_ARRIVAL),
                DBT_DEVICEREMOVECOMPLETE => queue_keyboard_device_notification(GIDC_REMOVAL),
                _ => {}
            }
        }
        return LRESULT(0);
    }

    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn retire_keyboard_hook(hook: &mut Option<HHOOK>) -> Result<(), WindowsError> {
    let Some(active_hook) = *hook else {
        return Ok(());
    };

    match unsafe { UnhookWindowsHookEx(active_hook) } {
        Ok(()) => {
            *hook = None;
            HOOK_INSTALLED.store(false, Ordering::Release);
            Ok(())
        }
        Err(error) if hook_is_already_gone(&error) => {
            *hook = None;
            HOOK_INSTALLED.store(false, Ordering::Release);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn hook_is_already_gone(error: &WindowsError) -> bool {
    WIN32_ERROR::from_error(error) == Some(ERROR_INVALID_HOOK_HANDLE)
}

fn restore_keyboard_hook(module: HINSTANCE, hook: &mut Option<HHOOK>) -> Result<(), WindowsError> {
    // There is no reliable WH_KEYBOARD_LL liveness query. Windows may silently remove a low-level
    // hook, so resume recovery always retires the old handle first. We only install a replacement
    // after retirement is confirmed, which prevents two live NumFlow hooks from coexisting.
    retire_keyboard_hook(hook)?;
    *hook = Some(install_keyboard_hook(module)?);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResumePhase {
    Automatic,
    User,
    SessionUnlock,
    DesktopReady,
}

impl ResumePhase {
    const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::User => "user",
            Self::SessionUnlock => "session-unlock",
            Self::DesktopReady => "desktop-ready",
        }
    }
}

fn run_message_loop(
    module: HINSTANCE,
    hook: &mut Option<HHOOK>,
    session_window: Option<HWND>,
) -> Result<(), HookError> {
    let mut message = MSG::default();

    loop {
        let result = unsafe { GetMessageW(&raw mut message, None, 0, 0) };
        match result.0 {
            -1 => return Err(HookError::MessageLoop(WindowsError::from_thread())),
            0 => return Ok(()),
            _ => {}
        }

        match message.message {
            WM_NUMFLOW_SUSPEND => handle_suspend_notification(),
            WM_NUMFLOW_RESUME_AUTOMATIC => {
                let _ = handle_resume_notification(
                    module,
                    hook,
                    session_window,
                    ResumePhase::Automatic,
                );
            }
            WM_NUMFLOW_RESUME_USER => {
                // PBT_APMRESUMESUSPEND is the late/user-visible power phase. Re-arm here, then let
                // WTS session notifications provide an even later desktop/session recovery point.
                let _ = handle_resume_notification(module, hook, session_window, ResumePhase::User);
            }
            WM_NUMFLOW_SESSION_UNLOCK => {
                let _ = handle_resume_notification(
                    module,
                    hook,
                    session_window,
                    ResumePhase::SessionUnlock,
                );
            }
            WM_NUMFLOW_DESKTOP_READY => {
                let _ = handle_resume_notification(
                    module,
                    hook,
                    session_window,
                    ResumePhase::DesktopReady,
                );
            }
            WM_NUMFLOW_FOREGROUND_CHANGED => {
                let hwnd = HWND(message.lParam.0 as *mut _);
                handle_foreground_notification(module, hook, session_window, hwnd);
            }
            WM_NUMFLOW_KEYBOARD_DEVICE_CHANGED => {
                let reason = u32::try_from(message.wParam.0).unwrap_or_default();
                handle_keyboard_device_notification(module, hook, session_window, reason);
            }
            WM_NUMFLOW_INPUT_RESYNC => {
                let Some(reason) = InputResyncReason::from_code(message.wParam.0) else {
                    eprintln!("NumFlow: ignored unknown input resync reason");
                    continue;
                };
                let outcome = resync_input_state(module, hook, session_window, reason);
                if reason == InputResyncReason::Startup {
                    if outcome.hook_restored {
                        INPUT_RUNTIME_STATE.store(INPUT_RUNTIME_RUNNING, Ordering::Release);
                    } else {
                        let _ =
                            dispatch_event(KeyboardHookEvent::InputUnavailable { reason }, true);
                    }
                }
            }
            _ => {
                // The hook thread now owns a message-only window for WM_WTSSESSION_CHANGE. Dispatch
                // ordinary window messages so its WNDPROC can translate session lifecycle events
                // into the private hook-thread recovery messages above.
                let _ = unsafe { DispatchMessageW(&raw const message) };
            }
        }
    }
}

fn handle_suspend_notification() {
    INPUT_RUNTIME_STATE.store(INPUT_RUNTIME_SUSPENDED, Ordering::Release);
    INTERCEPTION_ENABLED.store(false, Ordering::Release);
    NUM_LOCK_KEY_DOWN.store(false, Ordering::Release);
    RESUME_NUM_LOCK_GUARD.store(true, Ordering::Release);
    RESUME_WINDOWS_NUM_LOCK_MISMATCH.store(false, Ordering::Release);

    let cleanup = KeyboardHookEvent::NumLockChanged {
        num_lock_on: true,
        sync_system: false,
        play_feedback: false,
    };
    let _ = dispatch_lifecycle_events(&[cleanup]);
    lifecycle_log("suspend detected");
}

fn handle_resume_notification(
    module: HINSTANCE,
    hook: &mut Option<HHOOK>,
    session_window: Option<HWND>,
    phase: ResumePhase,
) -> bool {
    eprintln!("NumFlow: resume {} detected", phase.label());
    let reason = match phase {
        ResumePhase::Automatic => InputResyncReason::ResumeAutomatic,
        ResumePhase::User => InputResyncReason::ResumeUser,
        ResumePhase::SessionUnlock => InputResyncReason::SessionUnlock,
        ResumePhase::DesktopReady => InputResyncReason::DesktopReady,
    };
    let outcome = resync_input_state(module, hook, session_window, reason);
    let hook_restored = outcome.hook_restored;

    // `GetKeyState` reflects the calling thread's message-queue state. The hook thread is a
    // background worker, so after Sleep/Resume its toggle bit can lag behind the foreground app.
    // Keep the state tracked from real Num Lock transitions as the resume authority instead of
    // overwriting it with a foreground-dependent snapshot. The first physical NumPad event after
    // resume independently reconciles the mode from the VK/scan-code semantics reported by Windows.
    let tracked_num_lock_on = NUM_LOCK_ON.load(Ordering::Acquire);
    eprintln!(
        "NumFlow: NumLock resume state (tracked={tracked_num_lock_on}, source=last-confirmed-transition)"
    );

    let session_lock_pending = SESSION_LOCK_PENDING.load(Ordering::Acquire);
    let finalize_guard = resume_guard_should_finalize(phase, session_lock_pending);

    if finalize_guard && hook_restored {
        let sync_windows = RESUME_WINDOWS_NUM_LOCK_MISMATCH.swap(false, Ordering::AcqRel);
        let delivered = resync_runtime_num_lock(
            hook_restored,
            tracked_num_lock_on,
            phase.label(),
            sync_windows,
            reason,
        );

        if delivered {
            INPUT_RUNTIME_STATE.store(INPUT_RUNTIME_RUNNING, Ordering::Release);
            RESUME_NUM_LOCK_GUARD.store(false, Ordering::Release);
            if matches!(
                phase,
                ResumePhase::SessionUnlock | ResumePhase::DesktopReady
            ) {
                SESSION_LOCK_PENDING.store(false, Ordering::Release);
            }
            eprintln!(
                "NumFlow: resume NumLock lifecycle guard cleared (phase={}, tracked={tracked_num_lock_on})",
                phase.label()
            );
        }
    } else {
        // Re-arm the keyboard hook early, but do not reactivate pointer injection before Windows
        // has returned to an interactive session. Real hardware showed SendInput returning zero
        // between the user-visible power callback and WTS_SESSION_UNLOCK. Keeping interception
        // disabled here prevents a NumPad press on the transition/lock desktop from faulting the
        // background pointer runtime.
        INTERCEPTION_ENABLED.store(false, Ordering::Release);
        eprintln!(
            "NumFlow: pointer activation deferred (phase={}, session_lock_pending={session_lock_pending})",
            phase.label()
        );
    }

    if !hook_restored {
        let _ = dispatch_event(KeyboardHookEvent::InputUnavailable { reason }, true);
    }

    hook_restored
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputResyncOutcome {
    hook_restored: bool,
}

fn resync_input_state(
    module: HINSTANCE,
    hook: &mut Option<HHOOK>,
    _session_window: Option<HWND>,
    reason: InputResyncReason,
) -> InputResyncOutcome {
    let current_state = InputRuntimeState::from_raw(INPUT_RUNTIME_STATE.load(Ordering::Acquire));
    let should_rearm = should_rearm_input(reason, current_state, hook.is_some());

    if should_rearm {
        INPUT_RUNTIME_STATE.store(INPUT_RUNTIME_RECOVERING, Ordering::Release);
        INTERCEPTION_ENABLED.store(false, Ordering::Release);
        NUM_LOCK_KEY_DOWN.store(false, Ordering::Release);
    }

    let hook_restored = if should_rearm {
        match restore_keyboard_hook(module, hook) {
            Ok(()) => {
                eprintln!(
                    "NumFlow: hook restored (reason={}, generation={})",
                    reason.label(),
                    HOOK_GENERATION.load(Ordering::Acquire)
                );
                true
            }
            Err(error) => {
                eprintln!(
                    "NumFlow: hook restore failed (reason={}): {error}",
                    reason.label()
                );
                false
            }
        }
    } else {
        hook.is_some()
    };

    let device_notifications = KEYBOARD_DEVICE_NOTIFICATIONS_REGISTERED.load(Ordering::Acquire);
    let windows_num_lock_on = read_windows_num_lock_state();
    let tracked_num_lock_on = NUM_LOCK_ON.load(Ordering::Acquire);
    eprintln!(
        "NumFlow: input state resynchronized (reason={}, vk_numlock_snapshot={}, num_lock_on={}, numflow_enabled={}, hook_alive={}, raw_input_state={}, keyboard_device_notifications={})",
        reason.label(),
        windows_num_lock_on,
        tracked_num_lock_on,
        !tracked_num_lock_on,
        hook_restored,
        raw_input_state_label(),
        device_notifications,
    );
    eprintln!(
        "NumFlow: input health (hook alive = {}, hook callbacks = {}, raw_input_state={}, keyboard_device_notifications={})",
        hook_restored,
        HOOK_EVENT_COUNT.load(Ordering::Acquire),
        raw_input_state_label(),
        device_notifications,
    );
    log_input_snapshot(reason.label());

    InputResyncOutcome { hook_restored }
}

fn log_input_snapshot(reason: &str) {
    let current_process_id = unsafe { GetCurrentProcessId() };
    let foreground = crate::foreground_process_info();
    let foreground_scope = foreground.as_ref().map_or("other", |process| {
        if process.process_id == current_process_id {
            "NumFlow"
        } else {
            "other"
        }
    });
    let foreground_process = foreground
        .as_ref()
        .map_or("unavailable", |process| process.process_name.as_str());
    let num_lock_on = NUM_LOCK_ON.load(Ordering::Acquire);
    let raw_input_state = raw_input_state_label();

    eprintln!(
        "NumFlow: input snapshot (reason={reason}, foreground={foreground_scope}, foreground_process={foreground_process}, hook_alive={}, hook_generation={}, hook_callbacks={}, numpad_callbacks={}, numpad_dispatched={}, numpad_dropped={}, runtime_numpad_events={}, num_lock_on={num_lock_on}, numflow_enabled={}, interception={}, raw_input_state={raw_input_state}, keyboard_device_notifications={})",
        HOOK_INSTALLED.load(Ordering::Acquire),
        HOOK_GENERATION.load(Ordering::Acquire),
        HOOK_EVENT_COUNT.load(Ordering::Acquire),
        HOOK_NUMPAD_EVENT_COUNT.load(Ordering::Acquire),
        HOOK_NUMPAD_DISPATCHED_COUNT.load(Ordering::Acquire),
        HOOK_NUMPAD_DROPPED_COUNT.load(Ordering::Acquire),
        RUNTIME_NUMPAD_EVENT_COUNT.load(Ordering::Acquire),
        !num_lock_on,
        INTERCEPTION_ENABLED.load(Ordering::Acquire),
        KEYBOARD_DEVICE_NOTIFICATIONS_REGISTERED.load(Ordering::Acquire),
    );
}

fn raw_input_state_label() -> &'static str {
    if WINIT_RAW_KEYBOARD_REGISTRATION_DISABLED.load(Ordering::Acquire) {
        "keyboard-disabled"
    } else {
        "winit-owned"
    }
}

fn should_rearm_input(
    reason: InputResyncReason,
    _current_state: InputRuntimeState,
    hook_present: bool,
) -> bool {
    match reason {
        // Startup, foreground changes, and keyboard hotplug are health checkpoints, not proof that
        // Windows removed the hook. Reinstall only when the owning thread has actually lost its
        // local handle; this keeps focus changes side-effect free and prevents duplicate hooks.
        InputResyncReason::Startup
        | InputResyncReason::ForegroundChanged
        | InputResyncReason::KeyboardDeviceChanged => !hook_present,
        InputResyncReason::NumLockChanged => false,
        _ => true,
    }
}

fn read_windows_num_lock_state() -> bool {
    let key_state = unsafe { GetKeyState(i32::from(VK_NUMLOCK.0)) };
    key_state & 1 != 0
}

fn handle_foreground_notification(
    module: HINSTANCE,
    hook: &mut Option<HHOOK>,
    session_window: Option<HWND>,
    window: HWND,
) {
    if let Some(target) = crate::foreground_process_info_for_window(window) {
        eprintln!(
            "NumFlow: foreground changed -> {} (pid={}, integrity={}, elevated={:?})",
            target.process_name,
            target.process_id,
            target.integrity.unwrap_or("unknown"),
            target.elevated
        );
    } else {
        eprintln!("NumFlow: foreground changed -> <unavailable>");
    }

    let state = InputRuntimeState::from_raw(INPUT_RUNTIME_STATE.load(Ordering::Acquire));
    if matches!(state, InputRuntimeState::Suspended) || SESSION_LOCK_PENDING.load(Ordering::Acquire)
    {
        eprintln!("NumFlow: foreground change observed while input runtime is suspended");
        return;
    }

    if RESUME_NUM_LOCK_GUARD.load(Ordering::Acquire) {
        eprintln!("NumFlow: foreground change deferred until input recovery is finalized");
        return;
    }

    let outcome = resync_input_state(
        module,
        hook,
        session_window,
        InputResyncReason::ForegroundChanged,
    );
    if outcome.hook_restored
        && !matches!(state, InputRuntimeState::Running)
        && resync_runtime_num_lock(
            true,
            NUM_LOCK_ON.load(Ordering::Acquire),
            "foreground-changed",
            false,
            InputResyncReason::ForegroundChanged,
        )
    {
        INPUT_RUNTIME_STATE.store(INPUT_RUNTIME_RUNNING, Ordering::Release);
    } else if !outcome.hook_restored {
        let _ = dispatch_event(
            KeyboardHookEvent::InputUnavailable {
                reason: InputResyncReason::HookFailure,
            },
            true,
        );
    }
}

fn handle_keyboard_device_notification(
    module: HINSTANCE,
    hook: &mut Option<HHOOK>,
    session_window: Option<HWND>,
    reason: u32,
) {
    let label = match reason {
        GIDC_ARRIVAL => "arrival",
        GIDC_REMOVAL => "removal",
        _ => "unknown",
    };
    eprintln!("NumFlow: keyboard device changed -> {label}");

    if SESSION_LOCK_PENDING.load(Ordering::Acquire) || RESUME_NUM_LOCK_GUARD.load(Ordering::Acquire)
    {
        eprintln!("NumFlow: keyboard device change deferred until session recovery completes");
        return;
    }

    let outcome = resync_input_state(
        module,
        hook,
        session_window,
        InputResyncReason::KeyboardDeviceChanged,
    );
    let num_lock_on = NUM_LOCK_ON.load(Ordering::Acquire);
    if outcome.hook_restored
        && resync_runtime_num_lock(
            true,
            num_lock_on,
            "keyboard-device-changed",
            false,
            InputResyncReason::KeyboardDeviceChanged,
        )
    {
        INPUT_RUNTIME_STATE.store(INPUT_RUNTIME_RUNNING, Ordering::Release);
    } else if !outcome.hook_restored {
        let _ = dispatch_event(
            KeyboardHookEvent::InputUnavailable {
                reason: InputResyncReason::KeyboardDeviceChanged,
            },
            true,
        );
    }
}

fn resume_guard_should_finalize(phase: ResumePhase, session_lock_pending: bool) -> bool {
    match phase {
        ResumePhase::Automatic => false,
        ResumePhase::User => !session_lock_pending,
        ResumePhase::SessionUnlock | ResumePhase::DesktopReady => true,
    }
}

fn resync_runtime_num_lock(
    hook_restored: bool,
    num_lock_on: bool,
    phase: &str,
    sync_windows: bool,
    reason: InputResyncReason,
) -> bool {
    // The first event is a transient fail-safe cleanup. Runtime handling of NumLock ON resets the
    // normalizer, stops motion, and releases an active NumFlow mouse hold. The second event restores
    // the tracked Num Lock/NumFlow mode. Resume never replaces that mode with `GetKeyState` from the
    // background hook thread; physical NumPad semantics reconcile any real external toggle later.
    // No sleep or polling is involved.
    let cleanup = KeyboardHookEvent::NumLockChanged {
        num_lock_on: true,
        sync_system: false,
        play_feedback: false,
    };
    let restore = KeyboardHookEvent::NumLockChanged {
        num_lock_on,
        sync_system: sync_windows,
        play_feedback: false,
    };
    let unavailable = KeyboardHookEvent::InputUnavailable { reason };
    let delivered = if hook_restored {
        dispatch_lifecycle_events(&[cleanup, restore])
    } else {
        dispatch_lifecycle_events(&[cleanup, restore, unavailable])
    };

    if hook_restored && delivered {
        INTERCEPTION_ENABLED.store(!num_lock_on, Ordering::Release);
    } else {
        INTERCEPTION_ENABLED.store(false, Ordering::Release);
    }

    if delivered {
        eprintln!(
            "NumFlow: NumLock resynced (phase={}, num_lock_on={num_lock_on}, numflow_enabled={}, interception={})",
            phase,
            !num_lock_on,
            INTERCEPTION_ENABLED.load(Ordering::Acquire)
        );
    } else {
        eprintln!("NumFlow: NumLock resync event could not be delivered");
    }
    delivered
}

fn lifecycle_log(message: &str) {
    eprintln!("NumFlow: {message}");
}

#[cfg(test)]
fn power_notification_message(event_type: u32) -> Option<u32> {
    match event_type {
        PBT_APMSUSPEND => Some(WM_NUMFLOW_SUSPEND),
        PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMECRITICAL => Some(WM_NUMFLOW_RESUME_AUTOMATIC),
        PBT_APMRESUMESUSPEND => Some(WM_NUMFLOW_RESUME_USER),
        _ => None,
    }
}

fn ordered_power_notification(stage: u32, event_type: u32) -> (u32, Option<u32>) {
    match event_type {
        PBT_APMSUSPEND => {
            SESSION_RESUME_STAGE.store(SESSION_STAGE_IDLE, Ordering::Release);
            (RESUME_STAGE_IDLE, Some(WM_NUMFLOW_SUSPEND))
        }
        PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMECRITICAL if stage >= RESUME_STAGE_USER => {
            (stage, None)
        }
        PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMECRITICAL => (
            stage.max(RESUME_STAGE_AUTOMATIC),
            Some(WM_NUMFLOW_RESUME_AUTOMATIC),
        ),
        PBT_APMRESUMESUSPEND => (RESUME_STAGE_USER, Some(WM_NUMFLOW_RESUME_USER)),
        _ => (stage, None),
    }
}

fn ordered_session_notification(stage: u32, reason: u32) -> (u32, Option<u32>) {
    match reason {
        WTS_SESSION_LOCK => (SESSION_STAGE_IDLE, None),
        WTS_SESSION_UNLOCK if stage < SESSION_STAGE_UNLOCK => {
            (SESSION_STAGE_UNLOCK, Some(WM_NUMFLOW_SESSION_UNLOCK))
        }
        WTS_SESSION_DESKTOP_READY_REASON if stage < SESSION_STAGE_DESKTOP_READY => {
            (SESSION_STAGE_DESKTOP_READY, Some(WM_NUMFLOW_DESKTOP_READY))
        }
        _ => (stage, None),
    }
}

fn queue_session_notification(reason: u32) {
    let Ok(_order_guard) = POWER_NOTIFICATION_ORDER.lock() else {
        return;
    };

    let stage = SESSION_RESUME_STAGE.load(Ordering::Acquire);
    let (next_stage, message) = ordered_session_notification(stage, reason);

    if reason == WTS_SESSION_LOCK {
        SESSION_RESUME_STAGE.store(SESSION_STAGE_IDLE, Ordering::Release);
        SESSION_LOCK_PENDING.store(true, Ordering::Release);
        INPUT_RUNTIME_STATE.store(INPUT_RUNTIME_SUSPENDED, Ordering::Release);
        RESUME_NUM_LOCK_GUARD.store(true, Ordering::Release);
        RESUME_WINDOWS_NUM_LOCK_MISMATCH.store(false, Ordering::Release);
        NUM_LOCK_KEY_DOWN.store(false, Ordering::Release);
        INTERCEPTION_ENABLED.store(false, Ordering::Release);

        // Session lock can occur without a power suspend. Quiesce the runtime immediately so
        // pointer motion and NumFlow-owned holds cannot continue onto the lock/secure desktop,
        // while preserving NUM_LOCK_ON as the mode to restore after unlock.
        let cleanup = KeyboardHookEvent::NumLockChanged {
            num_lock_on: true,
            sync_system: false,
            play_feedback: false,
        };
        if !dispatch_lifecycle_events(&[cleanup]) {
            eprintln!("NumFlow: session-lock runtime quiesce event could not be delivered");
        }
        lifecycle_log("session lock detected; pointer runtime quiesced");
        return;
    }

    let Some(message) = message else {
        return;
    };
    let thread_id = HOOK_THREAD_ID.load(Ordering::Acquire);
    if thread_id == 0 {
        return;
    }

    match unsafe { PostThreadMessageW(thread_id, message, WPARAM(0), LPARAM(0)) } {
        Ok(()) => {
            SESSION_RESUME_STAGE.store(next_stage, Ordering::Release);
            // Once the interactive session is available, a delayed automatic power callback from
            // the same cycle is stale and must not regress the final session-level recovery.
            POWER_RESUME_STAGE.fetch_max(RESUME_STAGE_USER, Ordering::AcqRel);
        }
        Err(error) => eprintln!("NumFlow: failed to queue session lifecycle message: {error}"),
    }
}

unsafe extern "system" fn power_notification_callback(
    _context: *const c_void,
    event_type: u32,
    _setting: *const c_void,
) -> u32 {
    // RegisterSuspendResumeNotification callback mode can invoke callbacks independently of the
    // hook thread. Serialize the callback-to-message bridge so the documented resume phases cannot
    // be reordered by concurrent callback scheduling. Once the user-visible phase has been queued,
    // a delayed automatic callback from the same resume cycle is stale and must not regress the
    // final hook/runtime state.
    let Ok(_order_guard) = POWER_NOTIFICATION_ORDER.lock() else {
        return 0;
    };

    let stage = POWER_RESUME_STAGE.load(Ordering::Acquire);
    let (next_stage, message) = ordered_power_notification(stage, event_type);
    let Some(message) = message else {
        if matches!(event_type, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMECRITICAL)
            && stage >= RESUME_STAGE_USER
        {
            eprintln!("NumFlow: stale automatic resume callback ignored after user resume");
        }
        return 0;
    };

    let thread_id = HOOK_THREAD_ID.load(Ordering::Acquire);
    if thread_id == 0 {
        return 0;
    }

    match unsafe { PostThreadMessageW(thread_id, message, WPARAM(0), LPARAM(0)) } {
        Ok(()) => POWER_RESUME_STAGE.store(next_stage, Ordering::Release),
        Err(error) => eprintln!("NumFlow: failed to queue power lifecycle message: {error}"),
    }
    0
}

fn register_dispatcher(
    sender: Sender<KeyboardHookEvent>,
    overflow_reader: Receiver<KeyboardHookEvent>,
) -> bool {
    let dispatcher = EVENT_DISPATCHER.get_or_init(|| Mutex::new(None));
    let Ok(mut slot) = dispatcher.lock() else {
        return false;
    };
    if slot.is_some() {
        return false;
    }
    *slot = Some(HookDispatcher {
        sender,
        overflow_reader,
    });
    true
}

fn clear_dispatcher() {
    let Some(dispatcher) = EVENT_DISPATCHER.get() else {
        return;
    };
    if let Ok(mut slot) = dispatcher.lock() {
        *slot = None;
    }
}

fn dispatch_event(event: KeyboardHookEvent, priority: bool) -> bool {
    let Some(dispatcher) = EVENT_DISPATCHER.get() else {
        return false;
    };
    let Ok(slot) = dispatcher.try_lock() else {
        return false;
    };
    let Some(dispatcher) = slot.as_ref() else {
        return false;
    };

    match dispatcher.sender.try_send(event) {
        Ok(()) => true,
        Err(TrySendError::Full(event)) if priority => {
            let _ = dispatcher.overflow_reader.try_recv();
            dispatcher.sender.try_send(event).is_ok()
        }
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
    }
}

fn dispatch_lifecycle_events(events: &[KeyboardHookEvent]) -> bool {
    let Some(dispatcher) = EVENT_DISPATCHER.get() else {
        return false;
    };
    let Ok(slot) = dispatcher.lock() else {
        return false;
    };
    let Some(dispatcher) = slot.as_ref() else {
        return false;
    };

    // Pre-suspend key-up messages are not trustworthy after resume. Drop all stale keyboard work,
    // then enqueue the lifecycle sequence into a known-empty queue.
    while dispatcher.overflow_reader.try_recv().is_ok() {}

    for event in events {
        if dispatcher.sender.try_send(*event).is_err() {
            return false;
        }
    }
    true
}

fn num_lock_transition(
    num_lock_on: bool,
    key_down: bool,
    state: KeyState,
) -> (bool, bool, Option<bool>) {
    match state {
        KeyState::Pressed if !key_down => {
            let next = !num_lock_on;
            (next, true, Some(next))
        }
        KeyState::Pressed => (num_lock_on, true, None),
        KeyState::Released => (num_lock_on, false, None),
    }
}

fn observe_num_lock(state: KeyState) -> Option<bool> {
    let current = NUM_LOCK_ON.load(Ordering::Acquire);
    let key_down = NUM_LOCK_KEY_DOWN.load(Ordering::Acquire);
    let (next, next_key_down, changed) = num_lock_transition(current, key_down, state);
    NUM_LOCK_ON.store(next, Ordering::Release);
    NUM_LOCK_KEY_DOWN.store(next_key_down, Ordering::Release);

    if changed == Some(false) {
        // Num Lock OFF means pointer control. Start interception immediately in the hook so a
        // NumPad key pressed directly after Num Lock cannot leak through before the runtime wakes.
        INTERCEPTION_ENABLED.store(true, Ordering::Release);
    } else if changed == Some(true) {
        // Num Lock ON means normal number entry. Stop interception immediately; the runtime will
        // release any held pointer state while Windows processes the physical toggle normally.
        INTERCEPTION_ENABLED.store(false, Ordering::Release);
    }

    changed
}

fn dispatch_num_lock_change(
    state: KeyState,
    sync_system: bool,
    play_feedback: bool,
) -> Option<bool> {
    if RESUME_NUM_LOCK_GUARD.load(Ordering::Acquire) {
        if state == KeyState::Pressed {
            if sync_system {
                eprintln!(
                    "NumFlow: NumLock transition suppressed while resume lifecycle is frozen"
                );
            } else {
                let previous = RESUME_WINDOWS_NUM_LOCK_MISMATCH.fetch_xor(true, Ordering::AcqRel);
                eprintln!(
                    "NumFlow: external NumLock transition suppressed during frozen resume lifecycle (windows_mismatch={} -> {})",
                    previous, !previous
                );
            }
        }
        return None;
    }

    // UI commands and physical hook callbacks share one Num Lock transition order. The hook must
    // never wait behind a SendInput call from the runtime worker; if the short state transaction is
    // busy, pass this physical event through and let its next NumPad semantic reconcile the mode.
    let Ok(_state_guard) = NUM_LOCK_STATE_ORDER.try_lock() else {
        return None;
    };

    let changed = observe_num_lock(state);
    if let Some(num_lock_on) = changed {
        let _ = dispatch_event(
            KeyboardHookEvent::NumLockChanged {
                num_lock_on,
                sync_system,
                play_feedback,
            },
            true,
        );
    }
    changed
}

fn infer_num_lock_from_numpad(event: PhysicalKeyEvent) -> Option<bool> {
    if event.extended {
        return None;
    }

    match (event.scan_code, event.vk_code) {
        // Num Lock ON: Windows reports the physical digit keys as VK_NUMPAD0..VK_NUMPAD9.
        (0x52, 0x60)
        | (0x4F, 0x61)
        | (0x50, 0x62)
        | (0x51, 0x63)
        | (0x4B, 0x64)
        | (0x4C, 0x65)
        | (0x4D, 0x66)
        | (0x47, 0x67)
        | (0x48, 0x68)
        | (0x49, 0x69)
        | (0x53, 0x6E) => Some(true),

        // Num Lock OFF: the same physical scan codes are reported as navigation keys.
        (0x52, 0x2D) // Insert
        | (0x4F, 0x23) // End
        | (0x50, 0x28) // Down
        | (0x51, 0x22) // Page Down
        | (0x4B, 0x25) // Left
        | (0x4C, 0x0C) // Clear
        | (0x4D, 0x27) // Right
        | (0x47, 0x24) // Home
        | (0x48, 0x26) // Up
        | (0x49, 0x21) // Page Up
        | (0x53, 0x2E) => Some(false), // Delete
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumpadModeReconcileAction {
    AcceptObserved,
    PreserveTracked,
    PreserveTrackedAndSyncWindows,
}

fn numpad_mode_reconcile_action(
    tracked_num_lock_on: bool,
    observed_num_lock_on: bool,
    resume_guard: bool,
) -> NumpadModeReconcileAction {
    if !resume_guard {
        return NumpadModeReconcileAction::AcceptObserved;
    }
    if tracked_num_lock_on == observed_num_lock_on {
        NumpadModeReconcileAction::PreserveTracked
    } else {
        NumpadModeReconcileAction::PreserveTrackedAndSyncWindows
    }
}

fn reconcile_num_lock_from_numpad(event: PhysicalKeyEvent) {
    let Some(observed_num_lock_on) = infer_num_lock_from_numpad(event) else {
        return;
    };

    let Ok(_state_guard) = NUM_LOCK_STATE_ORDER.try_lock() else {
        return;
    };

    let tracked_num_lock_on = NUM_LOCK_ON.load(Ordering::Acquire);
    let action = numpad_mode_reconcile_action(
        tracked_num_lock_on,
        observed_num_lock_on,
        RESUME_NUM_LOCK_GUARD.load(Ordering::Acquire),
    );

    match action {
        NumpadModeReconcileAction::PreserveTracked => {
            INTERCEPTION_ENABLED.store(!tracked_num_lock_on, Ordering::Release);
            RESUME_WINDOWS_NUM_LOCK_MISMATCH.store(false, Ordering::Release);
        }
        NumpadModeReconcileAction::PreserveTrackedAndSyncWindows => {
            // During the lock-screen -> interactive-desktop transition Windows can temporarily
            // interpret the first physical NumPad key using a toggle state that differs from the
            // NumFlow mode tracked before suspend. Do not treat that transient semantic mismatch as
            // user intent. Keep the tracked mode, keep interception aligned with it, and ask the
            // normal runtime path to replay one tagged Num Lock toggle back to Windows. The same
            // physical NumPad event can still be handled immediately in NumFlow pointer mode.
            INTERCEPTION_ENABLED.store(!tracked_num_lock_on, Ordering::Release);
            RESUME_WINDOWS_NUM_LOCK_MISMATCH.store(true, Ordering::Release);
            eprintln!(
                "NumFlow: resume NumLock semantic mismatch (tracked={tracked_num_lock_on}, observed={observed_num_lock_on}); preserving tracked mode and deferring Windows resync until lifecycle finalization"
            );
        }
        NumpadModeReconcileAction::AcceptObserved => {
            let previous = NUM_LOCK_ON.swap(observed_num_lock_on, Ordering::AcqRel);
            if previous == observed_num_lock_on {
                return;
            }

            // Outside lifecycle recovery a physical NumPad event remains the strongest startup/
            // runtime signal for the actual Windows interpretation of that physical key.
            INTERCEPTION_ENABLED.store(!observed_num_lock_on, Ordering::Release);
            let _ = dispatch_event(
                KeyboardHookEvent::NumLockChanged {
                    num_lock_on: observed_num_lock_on,
                    sync_system: false,
                    play_feedback: false,
                },
                true,
            );
        }
    }
}

fn num_lock_replay_inputs() -> [INPUT; 2] {
    [
        num_lock_keyboard_input(KEYEVENTF_EXTENDEDKEY),
        num_lock_keyboard_input(KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
    ]
}

fn num_lock_keyboard_input(flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_NUMLOCK,
                wScan: NUM_LOCK_SCAN_CODE,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: NUMFLOW_NUM_LOCK_INJECTION_TAG,
            },
        },
    }
}

fn replay_num_lock_to_windows() -> bool {
    let inputs = num_lock_replay_inputs();
    let input_size = i32::try_from(size_of::<INPUT>()).expect("INPUT size fits in i32");
    let inserted = unsafe { SendInput(&inputs, input_size) };
    inserted == u32::try_from(inputs.len()).expect("Num Lock replay batch length fits in u32")
}

fn is_numflow_num_lock_replay(keyboard: KBDLLHOOKSTRUCT) -> bool {
    keyboard.flags.0 & LLKHF_INJECTED.0 != 0
        && keyboard.dwExtraInfo == NUMFLOW_NUM_LOCK_INJECTION_TAG
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        HOOK_EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
        let state = match u32::try_from(wparam.0).ok() {
            Some(WM_KEYDOWN | WM_SYSKEYDOWN) => Some(KeyState::Pressed),
            Some(WM_KEYUP | WM_SYSKEYUP) => Some(KeyState::Released),
            _ => None,
        };

        if let Some(state) = state {
            let keyboard = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };

            if keyboard.vkCode == u32::from(VK_NUMLOCK.0) {
                if is_numflow_num_lock_replay(keyboard) {
                    return unsafe { CallNextHookEx(None, code, wparam, lparam) };
                }

                if keyboard.flags.0 & LLKHF_INJECTED.0 != 0 {
                    // Respect Num Lock changes injected by other software and mirror them into
                    // NumFlow state, but do not consume somebody else's injected input.
                    let _ = dispatch_num_lock_change(state, false, true);
                    return unsafe { CallNextHookEx(None, code, wparam, lparam) };
                }

                // Do not consume the physical toggle. Windows owns the actual Num Lock state and
                // LED; NumFlow observes the edge before returning and derives its mode from that
                // same transition. This is also the safe fallback when SendInput is blocked by
                // UIPI or the input desktop is changing.
                let _ = dispatch_num_lock_change(state, false, true);
                return unsafe { CallNextHookEx(None, code, wparam, lparam) };
            }

            let event = PhysicalKeyEvent::new(
                keyboard.vkCode,
                keyboard.scanCode,
                keyboard.flags.0 & LLKHF_EXTENDED.0 != 0,
                state,
            );

            if map_numpad_key(event).is_some() {
                HOOK_NUMPAD_EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
                reconcile_num_lock_from_numpad(event);

                if INTERCEPTION_ENABLED.load(Ordering::Acquire)
                    && !NUM_LOCK_ON.load(Ordering::Acquire)
                {
                    if dispatch_event(KeyboardHookEvent::Key(event), false) {
                        HOOK_NUMPAD_DISPATCHED_COUNT.fetch_add(1, Ordering::Relaxed);
                        return LRESULT(1);
                    }

                    HOOK_NUMPAD_DROPPED_COUNT.fetch_add(1, Ordering::Relaxed);
                    eprintln!(
                        "NumFlow: NumPad event dispatch failed (scan_code={}, state={:?}, hook_alive={}, interception={}, num_lock_on={})",
                        event.scan_code,
                        event.state,
                        HOOK_INSTALLED.load(Ordering::Acquire),
                        INTERCEPTION_ENABLED.load(Ordering::Acquire),
                        NUM_LOCK_ON.load(Ordering::Acquire),
                    );
                }
            }
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use windows::{
        Win32::{
            Foundation::ERROR_INVALID_HOOK_HANDLE,
            UI::{
                Input::KeyboardAndMouse::{
                    INPUT_KEYBOARD, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_NUMLOCK,
                },
                Input::{RAWINPUTDEVICE, RIDEV_REMOVE},
                WindowsAndMessaging::{
                    DBT_DEVTYP_DEVICEINTERFACE, DEV_BROADCAST_DEVICEINTERFACE_W,
                    PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL, PBT_APMRESUMESUSPEND,
                    PBT_APMSUSPEND, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
                },
            },
        },
        core::Error as WindowsError,
    };

    use super::{
        GUID_DEVINTERFACE_KEYBOARD, INPUT_RUNTIME_RECOVERING, INPUT_RUNTIME_RUNNING,
        INPUT_RUNTIME_SUSPENDED, InputResyncReason, InputRuntimeState, NUM_LOCK_SCAN_CODE,
        NUMFLOW_NUM_LOCK_INJECTION_TAG, NumpadModeReconcileAction, RESUME_STAGE_AUTOMATIC,
        RESUME_STAGE_IDLE, RESUME_STAGE_USER, ResumePhase, SESSION_STAGE_DESKTOP_READY,
        SESSION_STAGE_IDLE, SESSION_STAGE_UNLOCK, WM_NUMFLOW_DESKTOP_READY,
        WM_NUMFLOW_RESUME_AUTOMATIC, WM_NUMFLOW_RESUME_USER, WM_NUMFLOW_SESSION_UNLOCK,
        WM_NUMFLOW_SUSPEND, WTS_SESSION_DESKTOP_READY_REASON, hook_is_already_gone,
        infer_num_lock_from_numpad, keyboard_device_notification_filter, num_lock_replay_inputs,
        num_lock_transition, numpad_mode_reconcile_action, ordered_power_notification,
        ordered_session_notification, power_notification_message, raw_keyboard_removal_device,
        resume_guard_should_finalize, should_rearm_input,
    };
    use crate::{KeyState, PhysicalKeyEvent};

    #[test]
    fn device_notification_filter_is_keyboard_interface_only() {
        let filter = keyboard_device_notification_filter();

        assert_eq!(
            filter.dbcc_size,
            u32::try_from(size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>())
                .expect("filter size should fit")
        );
        assert_eq!(filter.dbcc_devicetype, DBT_DEVTYP_DEVICEINTERFACE.0);
        assert_eq!(filter.dbcc_classguid, GUID_DEVINTERFACE_KEYBOARD);
    }

    #[test]
    fn raw_keyboard_removal_targets_only_the_generic_keyboard_class() {
        let device = raw_keyboard_removal_device();

        assert_eq!(device.usUsagePage, 0x01);
        assert_eq!(device.usUsage, 0x06);
        assert_eq!(device.dwFlags, RIDEV_REMOVE);
        assert!(device.hwndTarget.0.is_null());
        assert_eq!(
            size_of::<RAWINPUTDEVICE>(),
            std::mem::size_of_val(&device),
            "descriptor must use the Win32 ABI type"
        );
    }

    #[test]
    fn input_runtime_state_decodes_unknown_values_as_recovering() {
        assert_eq!(
            InputRuntimeState::from_raw(INPUT_RUNTIME_RUNNING),
            InputRuntimeState::Running
        );
        assert_eq!(
            InputRuntimeState::from_raw(INPUT_RUNTIME_SUSPENDED),
            InputRuntimeState::Suspended
        );
        assert_eq!(
            InputRuntimeState::from_raw(INPUT_RUNTIME_RECOVERING),
            InputRuntimeState::Recovering
        );
        assert_eq!(
            InputRuntimeState::from_raw(u32::MAX),
            InputRuntimeState::Recovering
        );
    }

    #[test]
    fn health_checkpoints_do_not_reinstall_a_present_low_level_hook() {
        for reason in [
            InputResyncReason::Startup,
            InputResyncReason::ForegroundChanged,
            InputResyncReason::KeyboardDeviceChanged,
        ] {
            assert!(!should_rearm_input(
                reason,
                InputRuntimeState::Running,
                true,
            ));
            assert!(!should_rearm_input(
                reason,
                InputRuntimeState::Recovering,
                true,
            ));
            assert!(should_rearm_input(
                reason,
                InputRuntimeState::Recovering,
                false,
            ));
        }
        assert!(should_rearm_input(
            InputResyncReason::SessionUnlock,
            InputRuntimeState::Suspended,
            true,
        ));
    }

    #[test]
    fn input_resync_reasons_round_trip_through_thread_message_codes() {
        let reasons = [
            InputResyncReason::Startup,
            InputResyncReason::ResumeAutomatic,
            InputResyncReason::ResumeUser,
            InputResyncReason::SessionUnlock,
            InputResyncReason::DesktopReady,
            InputResyncReason::ForegroundChanged,
            InputResyncReason::KeyboardDeviceChanged,
            InputResyncReason::HookFailure,
            InputResyncReason::NumLockChanged,
        ];

        for reason in reasons {
            assert_eq!(InputResyncReason::from_code(reason.code()), Some(reason));
        }
        assert_eq!(InputResyncReason::from_code(usize::MAX), None);
    }

    #[test]
    fn power_notifications_map_to_event_driven_hook_thread_messages() {
        assert_eq!(
            power_notification_message(PBT_APMSUSPEND),
            Some(WM_NUMFLOW_SUSPEND)
        );
        assert_eq!(
            power_notification_message(PBT_APMRESUMEAUTOMATIC),
            Some(WM_NUMFLOW_RESUME_AUTOMATIC)
        );
        assert_eq!(
            power_notification_message(PBT_APMRESUMECRITICAL),
            Some(WM_NUMFLOW_RESUME_AUTOMATIC)
        );
        assert_eq!(
            power_notification_message(PBT_APMRESUMESUSPEND),
            Some(WM_NUMFLOW_RESUME_USER)
        );
        assert_eq!(power_notification_message(u32::MAX), None);
    }

    #[test]
    fn resume_phase_labels_cover_all_recovery_stages() {
        assert_eq!(ResumePhase::Automatic.label(), "automatic");
        assert_eq!(ResumePhase::User.label(), "user");
        assert_eq!(ResumePhase::SessionUnlock.label(), "session-unlock");
        assert_eq!(ResumePhase::DesktopReady.label(), "desktop-ready");
    }

    #[test]
    fn user_resume_prevents_delayed_automatic_phase_regression() {
        let (stage, message) = ordered_power_notification(RESUME_STAGE_IDLE, PBT_APMRESUMESUSPEND);
        assert_eq!(stage, RESUME_STAGE_USER);
        assert_eq!(message, Some(WM_NUMFLOW_RESUME_USER));

        let (stage, message) = ordered_power_notification(stage, PBT_APMRESUMEAUTOMATIC);
        assert_eq!(stage, RESUME_STAGE_USER);
        assert_eq!(message, None);
    }

    #[test]
    fn automatic_then_user_resume_keeps_monotonic_phase_order() {
        let (stage, message) =
            ordered_power_notification(RESUME_STAGE_IDLE, PBT_APMRESUMEAUTOMATIC);
        assert_eq!(stage, RESUME_STAGE_AUTOMATIC);
        assert_eq!(message, Some(WM_NUMFLOW_RESUME_AUTOMATIC));

        let (stage, message) = ordered_power_notification(stage, PBT_APMRESUMESUSPEND);
        assert_eq!(stage, RESUME_STAGE_USER);
        assert_eq!(message, Some(WM_NUMFLOW_RESUME_USER));
    }

    #[test]
    fn session_unlock_and_desktop_ready_are_ordered_and_coalesced() {
        let (stage, message) = ordered_session_notification(SESSION_STAGE_IDLE, WTS_SESSION_UNLOCK);
        assert_eq!(stage, SESSION_STAGE_UNLOCK);
        assert_eq!(message, Some(WM_NUMFLOW_SESSION_UNLOCK));

        let (stage, message) = ordered_session_notification(stage, WTS_SESSION_UNLOCK);
        assert_eq!(stage, SESSION_STAGE_UNLOCK);
        assert_eq!(message, None);

        let (stage, message) =
            ordered_session_notification(stage, WTS_SESSION_DESKTOP_READY_REASON);
        assert_eq!(stage, SESSION_STAGE_DESKTOP_READY);
        assert_eq!(message, Some(WM_NUMFLOW_DESKTOP_READY));

        let (stage, message) =
            ordered_session_notification(stage, WTS_SESSION_DESKTOP_READY_REASON);
        assert_eq!(stage, SESSION_STAGE_DESKTOP_READY);
        assert_eq!(message, None);
    }

    #[test]
    fn session_lock_resets_session_recovery_cycle() {
        let (stage, message) =
            ordered_session_notification(SESSION_STAGE_DESKTOP_READY, WTS_SESSION_LOCK);
        assert_eq!(stage, SESSION_STAGE_IDLE);
        assert_eq!(message, None);

        let (stage, message) = ordered_session_notification(stage, WTS_SESSION_UNLOCK);
        assert_eq!(stage, SESSION_STAGE_UNLOCK);
        assert_eq!(message, Some(WM_NUMFLOW_SESSION_UNLOCK));
    }

    #[test]
    fn invalid_hook_handle_is_treated_as_already_retired() {
        let error: WindowsError = ERROR_INVALID_HOOK_HANDLE.into();
        assert!(hook_is_already_gone(&error));
    }

    #[test]
    fn num_lock_toggles_once_per_physical_press() {
        let (on, down, changed) = num_lock_transition(true, false, KeyState::Pressed);
        assert!(!on);
        assert!(down);
        assert_eq!(changed, Some(false));

        let (on, down, changed) = num_lock_transition(on, down, KeyState::Pressed);
        assert!(!on);
        assert!(down);
        assert_eq!(changed, None);

        let (on, down, changed) = num_lock_transition(on, down, KeyState::Released);
        assert!(!on);
        assert!(!down);
        assert_eq!(changed, None);

        let (on, down, changed) = num_lock_transition(on, down, KeyState::Pressed);
        assert!(on);
        assert!(down);
        assert_eq!(changed, Some(true));
    }

    #[test]
    fn infers_num_lock_on_from_physical_numpad_digit_semantics() {
        for (scan_code, vk_code) in [
            (0x52, 0x60),
            (0x4F, 0x61),
            (0x50, 0x62),
            (0x51, 0x63),
            (0x4B, 0x64),
            (0x4C, 0x65),
            (0x4D, 0x66),
            (0x47, 0x67),
            (0x48, 0x68),
            (0x49, 0x69),
            (0x53, 0x6E),
        ] {
            let event = PhysicalKeyEvent::new(vk_code, scan_code, false, KeyState::Pressed);
            assert_eq!(infer_num_lock_from_numpad(event), Some(true));
        }
    }

    #[test]
    fn infers_num_lock_off_from_physical_numpad_navigation_semantics() {
        for (scan_code, vk_code) in [
            (0x52, 0x2D),
            (0x4F, 0x23),
            (0x50, 0x28),
            (0x51, 0x22),
            (0x4B, 0x25),
            (0x4C, 0x0C),
            (0x4D, 0x27),
            (0x47, 0x24),
            (0x48, 0x26),
            (0x49, 0x21),
            (0x53, 0x2E),
        ] {
            let event = PhysicalKeyEvent::new(vk_code, scan_code, false, KeyState::Pressed);
            assert_eq!(infer_num_lock_from_numpad(event), Some(false));
        }
    }

    #[test]
    fn does_not_infer_num_lock_from_operator_or_extended_keys() {
        let add = PhysicalKeyEvent::new(0x6B, 0x4E, false, KeyState::Pressed);
        let navigation_cluster = PhysicalKeyEvent::new(0x28, 0x50, true, KeyState::Pressed);
        assert_eq!(infer_num_lock_from_numpad(add), None);
        assert_eq!(infer_num_lock_from_numpad(navigation_cluster), None);
    }

    #[test]
    fn resume_numpad_mismatch_preserves_tracked_mode_and_requests_windows_sync() {
        assert_eq!(
            numpad_mode_reconcile_action(false, true, true),
            NumpadModeReconcileAction::PreserveTrackedAndSyncWindows
        );
        assert_eq!(
            numpad_mode_reconcile_action(true, false, true),
            NumpadModeReconcileAction::PreserveTrackedAndSyncWindows
        );
    }

    #[test]
    fn resume_numpad_match_preserves_guard_until_lifecycle_finalization() {
        assert_eq!(
            numpad_mode_reconcile_action(false, false, true),
            NumpadModeReconcileAction::PreserveTracked
        );
        assert_eq!(
            numpad_mode_reconcile_action(true, true, true),
            NumpadModeReconcileAction::PreserveTracked
        );
    }

    #[test]
    fn locked_resume_guard_only_finalizes_at_session_or_desktop_ready() {
        assert!(!resume_guard_should_finalize(ResumePhase::Automatic, true));
        assert!(!resume_guard_should_finalize(ResumePhase::User, true));
        assert!(resume_guard_should_finalize(
            ResumePhase::SessionUnlock,
            true
        ));
        assert!(resume_guard_should_finalize(
            ResumePhase::DesktopReady,
            true
        ));
    }

    #[test]
    fn unlocked_power_resume_guard_finalizes_at_user_phase() {
        assert!(!resume_guard_should_finalize(ResumePhase::Automatic, false));
        assert!(resume_guard_should_finalize(ResumePhase::User, false));
    }

    #[test]
    fn ordinary_numpad_semantics_still_reconcile_outside_resume() {
        assert_eq!(
            numpad_mode_reconcile_action(false, true, false),
            NumpadModeReconcileAction::AcceptObserved
        );
    }

    #[test]
    fn num_lock_replay_is_tagged_keyboard_input() {
        let [down, up] = num_lock_replay_inputs();
        assert_eq!(down.r#type, INPUT_KEYBOARD);
        assert_eq!(up.r#type, INPUT_KEYBOARD);

        let down = unsafe { down.Anonymous.ki };
        let up = unsafe { up.Anonymous.ki };

        assert_eq!(down.wVk, VK_NUMLOCK);
        assert_eq!(up.wVk, VK_NUMLOCK);
        assert_eq!(down.wScan, NUM_LOCK_SCAN_CODE);
        assert_eq!(up.wScan, NUM_LOCK_SCAN_CODE);
        assert_eq!(down.dwFlags, KEYEVENTF_EXTENDEDKEY);
        assert_eq!(up.dwFlags, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP);
        assert_eq!(down.dwExtraInfo, NUMFLOW_NUM_LOCK_INJECTION_TAG);
        assert_eq!(up.dwExtraInfo, NUMFLOW_NUM_LOCK_INJECTION_TAG);
    }
}
