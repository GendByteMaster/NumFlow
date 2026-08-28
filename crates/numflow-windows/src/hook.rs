use std::{
    ffi::c_void,
    io,
    mem::size_of,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use windows::{
    Win32::{
        Foundation::{
            ERROR_INVALID_HOOK_HANDLE, HANDLE, HINSTANCE, LPARAM, LRESULT, WPARAM, WIN32_ERROR,
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            Power::{
                DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
                RegisterSuspendResumeNotification, UnregisterSuspendResumeNotification,
            },
            Threading::GetCurrentThreadId,
        },
        UI::{
            Input::{
                KeyboardAndMouse::{
                    GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
                    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, VK_NUMLOCK,
                },
                RAWINPUTDEVICE, RIDEV_REMOVE, RegisterRawInputDevices,
            },
            WindowsAndMessaging::{
                CallNextHookEx, DEVICE_NOTIFY_CALLBACK, GetMessageW, HHOOK, KBDLLHOOKSTRUCT,
                LLKHF_EXTENDED, LLKHF_INJECTED, MSG, PBT_APMRESUMEAUTOMATIC,
                PBT_APMRESUMECRITICAL, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PM_NOREMOVE,
                PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
                WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
            },
        },
    },
    core::Error as WindowsError,
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

static EVENT_DISPATCHER: OnceLock<Mutex<Option<HookDispatcher>>> = OnceLock::new();
static INTERCEPTION_ENABLED: AtomicBool = AtomicBool::new(false);
static NUM_LOCK_ON: AtomicBool = AtomicBool::new(true);
static NUM_LOCK_KEY_DOWN: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardHookEvent {
    Key(PhysicalKeyEvent),
    NumLockChanged {
        num_lock_on: bool,
        sync_system: bool,
        play_feedback: bool,
    },
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

fn raw_keyboard_removal_device() -> RAWINPUTDEVICE {
    RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_GENERIC,
        usUsage: HID_USAGE_GENERIC_KEYBOARD,
        dwFlags: RIDEV_REMOVE,
        ..RAWINPUTDEVICE::default()
    }
}

/// Removes the process-wide raw-keyboard device-event registration installed by winit.
///
/// Winit registers keyboards for raw `DeviceEvent` delivery on Windows. Windows can then stop
/// dispatching this process's `WH_KEYBOARD_LL` hook while one of the same process's windows owns
/// foreground focus. `NumFlow` does not consume winit raw `DeviceEvent::Key` events; Slint's normal
/// window keyboard handling continues through `WM_KEYDOWN` / `WM_KEYUP`. Removing only the raw
/// keyboard registration therefore keeps the UI keyboard-accessible while restoring `NumFlow`'s
/// global low-level hook inside its own focused settings window. Raw mouse registration is left
/// untouched.
///
/// This function is intentionally idempotent and should run after Slint/winit has initialized its
/// event loop. It is also safe to repeat after Windows resumes from sleep or hibernation.
///
/// # Errors
///
/// Returns the Win32 error from `RegisterRawInputDevices` if Windows rejects the removal request.
///
/// # Panics
///
/// Panics only if the compile-time `RAWINPUTDEVICE` size cannot fit in a Win32 `UINT`.
pub fn remove_raw_keyboard_device_event_registration() -> Result<(), WindowsError> {
    let device = raw_keyboard_removal_device();
    let device_size = u32::try_from(size_of::<RAWINPUTDEVICE>())
        .expect("RAWINPUTDEVICE size must fit in a Win32 UINT");

    unsafe { RegisterRawInputDevices(&[device], device_size) }
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

    let key_state = unsafe { GetKeyState(i32::from(VK_NUMLOCK.0)) };
    NUM_LOCK_ON.store(key_state & 1 != 0, Ordering::Release);
    NUM_LOCK_KEY_DOWN.store(key_state < 0, Ordering::Release);

    let module = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => HINSTANCE(module.0),
        Err(error) => {
            let _ = ready_sender.send(Err(HookError::Install(error)));
            return Ok(());
        }
    };

    let mut hook = match install_keyboard_hook(module) {
        Ok(hook) => Some(hook),
        Err(error) => {
            let _ = ready_sender.send(Err(HookError::Install(error)));
            return Ok(());
        }
    };

    if !register_dispatcher(event_sender, event_overflow_reader) {
        let _ = retire_keyboard_hook(&mut hook);
        let _ = ready_sender.send(Err(HookError::AlreadyActive));
        return Ok(());
    }

    HOOK_THREAD_ID.store(thread_id, Ordering::Release);
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

    if ready_sender.send(Ok(thread_id)).is_err() {
        HOOK_THREAD_ID.store(0, Ordering::Release);
        let _ = unsafe { UnregisterSuspendResumeNotification(power_registration) };
        clear_dispatcher();
        let _ = retire_keyboard_hook(&mut hook);
        return Ok(());
    }

    let loop_result = run_message_loop(module, &mut hook);

    INTERCEPTION_ENABLED.store(false, Ordering::Release);
    HOOK_THREAD_ID.store(0, Ordering::Release);
    let power_result = unsafe { UnregisterSuspendResumeNotification(power_registration) };
    clear_dispatcher();
    let unhook_result = retire_keyboard_hook(&mut hook);

    loop_result?;
    power_result.map_err(HookError::PowerNotification)?;
    unhook_result.map_err(HookError::Stop)
}

fn install_keyboard_hook(module: HINSTANCE) -> Result<HHOOK, WindowsError> {
    unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook_proc),
            Some(module),
            0,
        )
    }
}

fn register_suspend_resume_notifications() -> Result<HPOWERNOTIFY, HookError> {
    let parameters = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(power_notification_callback),
        Context: std::ptr::null_mut(),
    };
    let recipient = HANDLE(
        std::ptr::from_ref(&parameters)
            .cast::<c_void>()
            .cast_mut(),
    );

    unsafe { RegisterSuspendResumeNotification(recipient, DEVICE_NOTIFY_CALLBACK) }
        .map_err(HookError::PowerNotification)
}

fn retire_keyboard_hook(hook: &mut Option<HHOOK>) -> Result<(), WindowsError> {
    let Some(active_hook) = *hook else {
        return Ok(());
    };

    match unsafe { UnhookWindowsHookEx(active_hook) } {
        Ok(()) => {
            *hook = None;
            Ok(())
        }
        Err(error) if hook_is_already_gone(&error) => {
            *hook = None;
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

fn run_message_loop(module: HINSTANCE, hook: &mut Option<HHOOK>) -> Result<(), HookError> {
    let mut message = MSG::default();
    let mut automatic_resume_seen = false;
    let mut hook_restored = true;

    loop {
        let result = unsafe { GetMessageW(&raw mut message, None, 0, 0) };
        match result.0 {
            -1 => return Err(HookError::MessageLoop(WindowsError::from_thread())),
            0 => return Ok(()),
            _ => {}
        }

        match message.message {
            WM_NUMFLOW_SUSPEND => {
                automatic_resume_seen = false;
                hook_restored = true;
                handle_suspend_notification();
            }
            WM_NUMFLOW_RESUME_AUTOMATIC => {
                automatic_resume_seen = true;
                hook_restored = handle_resume_notification(module, hook);
            }
            WM_NUMFLOW_RESUME_USER => {
                // Windows normally emits RESUMEAUTOMATIC first and RESUMESUSPEND when the user
                // becomes active. Use the latter as an event-driven retry if re-arming failed, and
                // always reconcile winit Raw Input once more after the interactive session returns.
                if automatic_resume_seen && hook_restored {
                    reconcile_raw_keyboard_after_resume();
                } else {
                    hook_restored = handle_resume_notification(module, hook);
                }
            }
            _ => {}
        }
    }
}

fn handle_suspend_notification() {
    INTERCEPTION_ENABLED.store(false, Ordering::Release);
    NUM_LOCK_KEY_DOWN.store(false, Ordering::Release);

    let cleanup = KeyboardHookEvent::NumLockChanged {
        num_lock_on: true,
        sync_system: false,
        play_feedback: false,
    };
    let _ = dispatch_lifecycle_events(&[cleanup]);
    lifecycle_log("suspend detected");
}

fn handle_resume_notification(module: HINSTANCE, hook: &mut Option<HHOOK>) -> bool {
    INTERCEPTION_ENABLED.store(false, Ordering::Release);
    NUM_LOCK_KEY_DOWN.store(false, Ordering::Release);
    lifecycle_log("resume detected");

    let hook_restored = match restore_keyboard_hook(module, hook) {
        Ok(()) => {
            lifecycle_log("hook restored");
            true
        }
        Err(error) => {
            eprintln!("NumFlow: hook restore failed: {error}");
            false
        }
    };

    reconcile_raw_keyboard_after_resume();
    resync_runtime_num_lock(hook_restored);
    hook_restored
}

fn reconcile_raw_keyboard_after_resume() {
    if let Err(error) = remove_raw_keyboard_device_event_registration() {
        eprintln!("NumFlow: Raw Input reconciliation after resume failed: {error}");
    }
}

fn resync_runtime_num_lock(hook_restored: bool) {
    let num_lock_on = NUM_LOCK_ON.load(Ordering::Acquire);

    // The first event is a transient fail-safe cleanup. Runtime handling of NumLock ON resets the
    // normalizer, stops motion, and releases an active NumFlow mouse hold. The second event restores
    // the authoritative pre-suspend Num Lock/NumFlow mode. No sleep or polling is involved.
    let cleanup = KeyboardHookEvent::NumLockChanged {
        num_lock_on: true,
        sync_system: false,
        play_feedback: false,
    };
    let restore = KeyboardHookEvent::NumLockChanged {
        num_lock_on,
        sync_system: false,
        play_feedback: false,
    };
    let delivered = dispatch_lifecycle_events(&[cleanup, restore]);

    if hook_restored && delivered {
        INTERCEPTION_ENABLED.store(!num_lock_on, Ordering::Release);
    } else {
        INTERCEPTION_ENABLED.store(false, Ordering::Release);
    }

    if delivered {
        eprintln!(
            "NumFlow: NumLock resynced (num_lock_on={num_lock_on}, numflow_enabled={})",
            !num_lock_on
        );
    } else {
        eprintln!("NumFlow: NumLock resync event could not be delivered");
    }
}

fn lifecycle_log(message: &str) {
    eprintln!("NumFlow: {message}");
}

fn power_notification_message(event_type: u32) -> Option<u32> {
    match event_type {
        PBT_APMSUSPEND => Some(WM_NUMFLOW_SUSPEND),
        PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMECRITICAL => Some(WM_NUMFLOW_RESUME_AUTOMATIC),
        PBT_APMRESUMESUSPEND => Some(WM_NUMFLOW_RESUME_USER),
        _ => None,
    }
}

unsafe extern "system" fn power_notification_callback(
    _context: *const c_void,
    event_type: u32,
    _setting: *const c_void,
) -> u32 {
    let Some(message) = power_notification_message(event_type) else {
        return 0;
    };
    let thread_id = HOOK_THREAD_ID.load(Ordering::Acquire);
    if thread_id == 0 {
        return 0;
    }

    let _ = unsafe { PostThreadMessageW(thread_id, message, WPARAM(0), LPARAM(0)) };
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
        // release any held pointer state and then synchronize the Windows lock state.
        INTERCEPTION_ENABLED.store(false, Ordering::Release);
    }

    changed
}

fn dispatch_num_lock_change(
    state: KeyState,
    sync_system: bool,
    play_feedback: bool,
) -> Option<bool> {
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

fn reconcile_num_lock_from_numpad(event: PhysicalKeyEvent) {
    let Some(observed_num_lock_on) = infer_num_lock_from_numpad(event) else {
        return;
    };

    let previous = NUM_LOCK_ON.swap(observed_num_lock_on, Ordering::AcqRel);
    if previous == observed_num_lock_on {
        return;
    }

    // GetKeyState is thread-message-queue based and can be stale on a newly-created background
    // hook thread. A physical NumPad event carries the actual Windows interpretation, so use it to
    // repair startup state before deciding whether this same event should be intercepted.
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

fn intercept_physical_num_lock(state: KeyState) -> bool {
    // Always consume the physical Num Lock sequence. The runtime performs the tagged Windows
    // replay after this low-level hook callback returns, avoiding re-entrant SendInput here.
    let _ = dispatch_num_lock_change(state, true, true);
    true
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
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

                if intercept_physical_num_lock(state) {
                    return LRESULT(1);
                }

                return unsafe { CallNextHookEx(None, code, wparam, lparam) };
            }

            let event = PhysicalKeyEvent::new(
                keyboard.vkCode,
                keyboard.scanCode,
                keyboard.flags.0 & LLKHF_EXTENDED.0 != 0,
                state,
            );

            if map_numpad_key(event).is_some() {
                reconcile_num_lock_from_numpad(event);

                if INTERCEPTION_ENABLED.load(Ordering::Acquire)
                    && !NUM_LOCK_ON.load(Ordering::Acquire)
                    && dispatch_event(KeyboardHookEvent::Key(event), false)
                {
                    return LRESULT(1);
                }
            }
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use windows::{
        Win32::{
            Foundation::ERROR_INVALID_HOOK_HANDLE,
            UI::{
                Input::{
                    KeyboardAndMouse::{
                        INPUT_KEYBOARD, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_NUMLOCK,
                    },
                    RIDEV_REMOVE,
                },
                WindowsAndMessaging::{
                    PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL, PBT_APMRESUMESUSPEND,
                    PBT_APMSUSPEND,
                },
            },
        },
        core::Error as WindowsError,
    };

    use super::{
        HID_USAGE_GENERIC_KEYBOARD, HID_USAGE_PAGE_GENERIC, NUM_LOCK_SCAN_CODE,
        NUMFLOW_NUM_LOCK_INJECTION_TAG, WM_NUMFLOW_RESUME_AUTOMATIC, WM_NUMFLOW_RESUME_USER,
        WM_NUMFLOW_SUSPEND, hook_is_already_gone, infer_num_lock_from_numpad,
        num_lock_replay_inputs, num_lock_transition, power_notification_message,
        raw_keyboard_removal_device,
    };
    use crate::{KeyState, PhysicalKeyEvent};

    #[test]
    fn raw_keyboard_removal_descriptor_does_not_touch_mouse_registration() {
        let device = raw_keyboard_removal_device();

        assert_eq!(device.usUsagePage, HID_USAGE_PAGE_GENERIC);
        assert_eq!(device.usUsage, HID_USAGE_GENERIC_KEYBOARD);
        assert_eq!(device.dwFlags, RIDEV_REMOVE);
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
