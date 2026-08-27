#[cfg(not(windows))]
use numflow_core::InputAction;
use numflow_core::{Bindings, CoreEffect, MotionConfig, MouseButton};

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub motion: MotionConfig,
    pub bindings: Bindings,
    pub selected_button: MouseButton,
    pub precision: bool,
}

impl RuntimeConfig {
    #[must_use]
    pub fn new(
        motion: MotionConfig,
        bindings: Bindings,
        selected_button: MouseButton,
        precision: bool,
    ) -> Self {
        Self {
            motion,
            bindings,
            selected_button,
            precision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeStateSnapshot {
    pub enabled: bool,
    pub selected_button: MouseButton,
    pub held_button: Option<MouseButton>,
    pub precision: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeEvent {
    Effects {
        state: RuntimeStateSnapshot,
        effects: Vec<CoreEffect>,
    },
    Fault {
        state: RuntimeStateSnapshot,
        reason: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("failed to start NumFlow background runtime: {0}")]
    Start(String),
    #[error("NumFlow background runtime command channel is closed")]
    CommandChannelClosed,
    #[error("NumFlow background runtime command queue is full")]
    CommandQueueFull,
    #[error("NumFlow background runtime worker panicked")]
    WorkerPanicked,
}

#[cfg(windows)]
mod platform {
    use std::{
        sync::mpsc::{self, Receiver as StdReceiver, SyncSender},
        thread::{self, JoinHandle},
        time::{Duration, Instant},
    };

    use crossbeam_channel::{Receiver, Sender, TrySendError};
    use numflow_core::{
        ClickKind, ControllerState, CoreEffect, InputAction, MotionEngine, MotionModifiers,
        NumpadKey, PointerBackend, PointerEffect,
    };
    use numflow_windows::{
        AudioCue, AudioFeedbackService, KeyState, KeyboardEventNormalizer, KeyboardHook,
        KeyboardHookEvent, NormalizedKeyEvent, WindowsPointer,
    };

    use super::{RuntimeConfig, RuntimeError, RuntimeEvent, RuntimeStateSnapshot};

    const MOTION_TICK: Duration = Duration::from_millis(8);
    const COMMAND_QUEUE_CAPACITY: usize = 64;
    const EVENT_QUEUE_CAPACITY: usize = 64;
    const KEYBOARD_HOOK_START_ATTEMPTS: usize = 3;
    const KEYBOARD_HOOK_RETRY_DELAY: Duration = Duration::from_millis(100);

    #[derive(Debug)]
    enum RuntimeCommand {
        Apply(InputAction),
        Configure(RuntimeConfig),
        SetMotionConfig(numflow_core::MotionConfig),
        SetBindings(numflow_core::Bindings),
        Shutdown,
    }

    #[derive(Debug)]
    struct RuntimeEventSink {
        events: Sender<RuntimeEvent>,
        overflow_reader: Receiver<RuntimeEvent>,
        wake: SyncSender<()>,
    }

    impl RuntimeEventSink {
        fn send(&self, event: RuntimeEvent) {
            let delivered = match self.events.try_send(event) {
                Ok(()) => true,
                Err(TrySendError::Full(event)) => {
                    // The worker is the only producer. Evicting one stale UI event is therefore
                    // sufficient to make room without ever blocking pointer/input processing.
                    let _ = self.overflow_reader.try_recv();
                    self.events.try_send(event).is_ok()
                }
                Err(TrySendError::Disconnected(_)) => false,
            };

            if delivered {
                let _ = self.wake.try_send(());
            }
        }
    }

    #[derive(Debug)]
    pub struct BackgroundRuntime {
        command_sender: Sender<RuntimeCommand>,
        event_receiver: Receiver<RuntimeEvent>,
        wake_receiver: Option<StdReceiver<()>>,
        join: Option<JoinHandle<()>>,
    }

    impl BackgroundRuntime {
        pub fn start(config: RuntimeConfig) -> Result<Self, RuntimeError> {
            let (command_sender, command_receiver) =
                crossbeam_channel::bounded(COMMAND_QUEUE_CAPACITY);
            let (event_sender, event_receiver) = crossbeam_channel::bounded(EVENT_QUEUE_CAPACITY);
            let event_overflow_reader = event_receiver.clone();
            let (wake_sender, wake_receiver) = mpsc::sync_channel(1);
            let (ready_sender, ready_receiver) = mpsc::sync_channel(1);

            let join = thread::Builder::new()
                .name("numflow-runtime".to_owned())
                .spawn(move || {
                    let event_sink = RuntimeEventSink {
                        events: event_sender,
                        overflow_reader: event_overflow_reader,
                        wake: wake_sender,
                    };
                    worker_main(config, &command_receiver, &event_sink, &ready_sender);
                })
                .map_err(|error| RuntimeError::Start(error.to_string()))?;

            match ready_receiver.recv() {
                Ok(Ok(())) => Ok(Self {
                    command_sender,
                    event_receiver,
                    wake_receiver: Some(wake_receiver),
                    join: Some(join),
                }),
                Ok(Err(error)) => {
                    let _ = join.join();
                    Err(RuntimeError::Start(error))
                }
                Err(error) => {
                    let _ = join.join();
                    Err(RuntimeError::Start(error.to_string()))
                }
            }
        }

        pub fn apply(&self, action: InputAction) -> Result<(), RuntimeError> {
            self.send(RuntimeCommand::Apply(action))
        }

        pub fn configure(&self, config: RuntimeConfig) -> Result<(), RuntimeError> {
            self.send(RuntimeCommand::Configure(config))
        }

        pub fn set_motion_config(
            &self,
            config: numflow_core::MotionConfig,
        ) -> Result<(), RuntimeError> {
            self.send(RuntimeCommand::SetMotionConfig(config))
        }

        pub fn set_bindings(&self, bindings: numflow_core::Bindings) -> Result<(), RuntimeError> {
            self.send(RuntimeCommand::SetBindings(bindings))
        }

        #[must_use]
        pub fn drain_events(&self) -> Vec<RuntimeEvent> {
            let mut events = Vec::new();
            while let Ok(event) = self.event_receiver.try_recv() {
                events.push(event);
            }
            events
        }

        #[must_use]
        pub fn take_wake_receiver(&mut self) -> Option<StdReceiver<()>> {
            self.wake_receiver.take()
        }

        pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
            let Some(join) = self.join.take() else {
                return Ok(());
            };

            let _ = self.command_sender.send(RuntimeCommand::Shutdown);
            join.join().map_err(|_| RuntimeError::WorkerPanicked)
        }

        fn send(&self, command: RuntimeCommand) -> Result<(), RuntimeError> {
            match self.command_sender.try_send(command) {
                Ok(()) => Ok(()),
                Err(TrySendError::Full(_)) => Err(RuntimeError::CommandQueueFull),
                Err(TrySendError::Disconnected(_)) => Err(RuntimeError::CommandChannelClosed),
            }
        }
    }

    impl Drop for BackgroundRuntime {
        fn drop(&mut self) {
            let _ = self.shutdown();
        }
    }

    struct RuntimeMachine<B: PointerBackend> {
        controller: ControllerState,
        motion: MotionEngine,
        bindings: numflow_core::Bindings,
        pointer: B,
    }

    impl<B> RuntimeMachine<B>
    where
        B: PointerBackend,
    {
        fn new(config: RuntimeConfig, pointer: B) -> Self {
            let mut controller = ControllerState::default();
            let _ = controller.apply(InputAction::SelectButton(config.selected_button));
            let _ = controller.apply(InputAction::SetPrecision(config.precision));

            Self {
                controller,
                motion: MotionEngine::new(config.motion),
                bindings: config.bindings,
                pointer,
            }
        }

        fn enabled(&self) -> bool {
            self.controller.is_enabled()
        }

        fn snapshot(&self) -> RuntimeStateSnapshot {
            RuntimeStateSnapshot {
                enabled: self.controller.is_enabled(),
                selected_button: self.controller.selected_button(),
                held_button: self.controller.held_button(),
                precision: self.controller.is_precision_enabled(),
            }
        }

        fn configure(&mut self, config: RuntimeConfig) -> Result<(), B::Error> {
            self.motion.stop();
            self.motion.set_config(config.motion);
            self.bindings = config.bindings;
            let effects = self
                .controller
                .apply(InputAction::SelectButton(config.selected_button));
            self.execute_effects(&effects)?;
            let effects = self
                .controller
                .apply(InputAction::SetPrecision(config.precision));
            self.execute_effects(&effects)
        }

        fn set_motion_config(&mut self, config: numflow_core::MotionConfig) {
            self.motion.set_config(config);
        }

        fn set_bindings(&mut self, bindings: numflow_core::Bindings) {
            self.motion.stop();
            self.bindings = bindings;
        }

        fn apply_action(&mut self, action: InputAction) -> Result<Vec<CoreEffect>, B::Error> {
            match action {
                InputAction::Hold => return self.hold_selected_button(),
                InputAction::Release => return self.release_tracked_buttons(),
                InputAction::SetEnabled(false) if self.controller.is_enabled() => {
                    self.motion.stop();
                    let mut effects = self.release_tracked_buttons()?;
                    effects.extend(self.controller.apply(InputAction::SetEnabled(false)));
                    return Ok(effects);
                }
                _ => {}
            }

            let effects = self.controller.apply(action);
            if !self.controller.is_enabled() {
                self.motion.stop();
            }
            self.execute_effects(&effects)?;
            Ok(effects)
        }

        fn hold_selected_button(&mut self) -> Result<Vec<CoreEffect>, B::Error> {
            if !self.controller.is_enabled() || self.controller.held_button().is_some() {
                return Ok(Vec::new());
            }

            // Sweep any stale backend-only state before starting a new latch. WindowsPointer only
            // tracks buttons injected by NumFlow, so this never releases a physical button that the
            // user is holding directly on the mouse.
            self.pointer.release_all()?;
            let button = self.controller.selected_button();
            self.pointer.button_down(button)?;

            // Commit the controller state only after the physical Mouse Down succeeded. The
            // returned PointerEffect is for UI/HUD state propagation and must not be executed again.
            Ok(self.controller.apply(InputAction::Hold))
        }

        fn release_tracked_buttons(&mut self) -> Result<Vec<CoreEffect>, B::Error> {
            // Release every button still tracked by the backend first. This is intentionally wider
            // than the controller's single primary hold and repairs stale/multiple backend state.
            // The controller is cleared only after Windows accepted the release sequence.
            self.pointer.release_all()?;
            Ok(self.controller.apply(InputAction::Release))
        }

        fn handle_key_event(
            &mut self,
            event: NormalizedKeyEvent,
        ) -> Result<Vec<CoreEffect>, B::Error> {
            if let InputAction::Move(direction) = event.action {
                match event.state {
                    KeyState::Pressed if self.controller.is_enabled() => {
                        self.motion.press(direction);
                    }
                    KeyState::Released => self.motion.release(direction),
                    KeyState::Pressed => {}
                }
                return Ok(Vec::new());
            }

            if event.state == KeyState::Pressed {
                // NumPad 0 latches whichever mouse button is currently selected. Repeated presses
                // while a hold is active are idempotent in hold_selected_button().
                if event.key == NumpadKey::Num0 && event.action == InputAction::Hold {
                    return self.apply_action(InputAction::Hold);
                }

                if matches!(
                    event.key,
                    NumpadKey::Num5 | NumpadKey::Add | NumpadKey::Decimal
                ) {
                    let had_active_hold = self.controller.held_button().is_some();
                    let release_effects = self.release_tracked_buttons()?;

                    // Decimal / keypad Del is a dedicated release command. Num5 and + become
                    // release commands only while a hold is active; otherwise their configured
                    // click/double-click actions continue to work normally.
                    if had_active_hold || event.key == NumpadKey::Decimal {
                        return Ok(release_effects);
                    }
                }

                if matches!(
                    event.action,
                    InputAction::ToggleEnabled | InputAction::SetEnabled(_)
                ) {
                    return Ok(Vec::new());
                }
                self.apply_action(event.action)
            } else {
                Ok(Vec::new())
            }
        }

        fn tick(&mut self, elapsed: Duration) -> Result<(), B::Error> {
            if !self.controller.is_enabled() {
                self.motion.stop();
                return Ok(());
            }

            let modifiers = MotionModifiers {
                precision: self.controller.is_precision_enabled(),
                boost: false,
            };
            if let Some(step) = self.motion.tick(elapsed, modifiers) {
                self.pointer.move_relative(step.dx, step.dy)?;
            }
            Ok(())
        }

        fn shutdown(&mut self) -> Result<Vec<CoreEffect>, B::Error> {
            self.motion.stop();
            self.pointer.release_all()?;
            let mut effects = self.controller.apply(InputAction::Release);
            effects.extend(self.controller.shutdown());
            Ok(effects)
        }

        fn execute_effects(&mut self, effects: &[CoreEffect]) -> Result<(), B::Error> {
            for effect in effects {
                let CoreEffect::Pointer(pointer_effect) = effect else {
                    continue;
                };
                match pointer_effect {
                    PointerEffect::Move(_) => {}
                    PointerEffect::Click { button, kind } => match kind {
                        ClickKind::Single => self.pointer.click(*button)?,
                        ClickKind::Double => self.pointer.double_click(*button)?,
                    },
                    PointerEffect::ButtonDown(button) => self.pointer.button_down(*button)?,
                    PointerEffect::ButtonUp(button) => self.pointer.button_up(*button)?,
                }
            }
            Ok(())
        }
    }

    fn start_keyboard_hook_with_retry()
    -> Result<(KeyboardHook, Receiver<KeyboardHookEvent>), String> {
        let mut last_error = None;

        for attempt in 1..=KEYBOARD_HOOK_START_ATTEMPTS {
            match KeyboardHook::start() {
                Ok(runtime) => {
                    tracing::info!(attempt, "NumFlow keyboard hook registered and ready");
                    return Ok(runtime);
                }
                Err(error) => {
                    let error = error.to_string();
                    tracing::warn!(
                        attempt,
                        attempts = KEYBOARD_HOOK_START_ATTEMPTS,
                        %error,
                        "failed to initialize NumFlow keyboard hook"
                    );
                    last_error = Some(error);
                    if attempt < KEYBOARD_HOOK_START_ATTEMPTS {
                        thread::sleep(KEYBOARD_HOOK_RETRY_DELAY);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "keyboard hook initialization failed".to_owned()))
    }

    fn worker_main(
        config: RuntimeConfig,
        command_receiver: &Receiver<RuntimeCommand>,
        event_sink: &RuntimeEventSink,
        ready_sender: &mpsc::SyncSender<Result<(), String>>,
    ) {
        let (hook, keyboard_receiver) = match start_keyboard_hook_with_retry() {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = ready_sender.send(Err(error));
                return;
            }
        };
        hook.set_interception_enabled(false);

        let audio_feedback = match AudioFeedbackService::start() {
            Ok(service) => Some(service),
            Err(error) => {
                tracing::warn!(%error, "NumFlow audio feedback is unavailable");
                None
            }
        };
        let mut normalizer = KeyboardEventNormalizer::default();
        let mut machine = RuntimeMachine::new(config, WindowsPointer::default());
        let startup_effects = match apply_num_lock_mode(&mut machine, hook.num_lock_on()) {
            Ok(effects) => effects,
            Err(error) => {
                let _ = ready_sender.send(Err(error.to_string()));
                return;
            }
        };
        hook.set_interception_enabled(machine.enabled());
        if !startup_effects.is_empty() {
            event_sink.send(RuntimeEvent::Effects {
                state: machine.snapshot(),
                effects: startup_effects,
            });
        }
        let _ = ready_sender.send(Ok(()));
        let motion_tick = crossbeam_channel::tick(MOTION_TICK);
        let mut previous_tick = Instant::now();
        let mut running = true;

        while running {
            if machine.motion.is_moving() {
                crossbeam_channel::select! {
                    recv(command_receiver) -> command => {
                        running = handle_command_message(
                            command,
                            &mut machine,
                            &hook,
                            &mut normalizer,
                            event_sink,
                        );
                    }
                    recv(keyboard_receiver) -> event => {
                        running = handle_keyboard_message(
                            event,
                            &mut machine,
                            &hook,
                            &mut normalizer,
                            event_sink,
                            audio_feedback.as_ref(),
                        );
                    }
                    recv(motion_tick) -> _ => {
                        let now = Instant::now();
                        let elapsed = now.saturating_duration_since(previous_tick);
                        previous_tick = now;
                        if let Err(error) = machine.tick(elapsed) {
                            fail_safe(
                                &mut machine,
                                &hook,
                                &mut normalizer,
                                event_sink,
                                &error.to_string(),
                            );
                        }
                    }
                }
            } else {
                crossbeam_channel::select! {
                    recv(command_receiver) -> command => {
                        running = handle_command_message(
                            command,
                            &mut machine,
                            &hook,
                            &mut normalizer,
                            event_sink,
                        );
                    }
                    recv(keyboard_receiver) -> event => {
                        running = handle_keyboard_message(
                            event,
                            &mut machine,
                            &hook,
                            &mut normalizer,
                            event_sink,
                            audio_feedback.as_ref(),
                        );
                    }
                }
                previous_tick = Instant::now();
            }
        }
    }

    fn handle_command_message(
        command: Result<RuntimeCommand, crossbeam_channel::RecvError>,
        machine: &mut RuntimeMachine<WindowsPointer>,
        hook: &KeyboardHook,
        normalizer: &mut KeyboardEventNormalizer,
        event_sink: &RuntimeEventSink,
    ) -> bool {
        match command {
            Ok(RuntimeCommand::Shutdown) => {
                let effects = match machine.shutdown() {
                    Ok(effects) => effects,
                    Err(error) => {
                        event_sink.send(RuntimeEvent::Fault {
                            state: machine.snapshot(),
                            reason: error.to_string(),
                        });
                        Vec::new()
                    }
                };
                if !effects.is_empty() {
                    event_sink.send(RuntimeEvent::Effects {
                        state: machine.snapshot(),
                        effects,
                    });
                }
                hook.emergency_disable();
                false
            }
            Ok(command) => {
                if let Err(error) = apply_command(command, machine, hook, normalizer) {
                    fail_safe(machine, hook, normalizer, event_sink, &error);
                }
                true
            }
            Err(_) => {
                let _ = machine.shutdown();
                hook.emergency_disable();
                false
            }
        }
    }

    fn handle_keyboard_message(
        event: Result<KeyboardHookEvent, crossbeam_channel::RecvError>,
        machine: &mut RuntimeMachine<WindowsPointer>,
        hook: &KeyboardHook,
        normalizer: &mut KeyboardEventNormalizer,
        event_sink: &RuntimeEventSink,
        audio_feedback: Option<&AudioFeedbackService>,
    ) -> bool {
        let Ok(event) = event else {
            fail_safe(
                machine,
                hook,
                normalizer,
                event_sink,
                "keyboard hook event channel disconnected",
            );
            return false;
        };

        let event = match event {
            KeyboardHookEvent::NumLockChanged {
                num_lock_on,
                sync_system,
                play_feedback,
            } => {
                normalizer.reset();
                if play_feedback && let Some(audio_feedback) = audio_feedback {
                    audio_feedback.play(if num_lock_on {
                        AudioCue::NumFlowOff
                    } else {
                        AudioCue::NumFlowOn
                    });
                }

                match apply_num_lock_mode(machine, num_lock_on) {
                    Ok(effects) => {
                        if sync_system && !hook.sync_num_lock_to_windows() {
                            tracing::warn!(
                                num_lock_on,
                                "failed to replay intercepted Num Lock toggle to Windows"
                            );
                        }
                        hook.set_interception_enabled(machine.enabled());
                        event_sink.send(RuntimeEvent::Effects {
                            state: machine.snapshot(),
                            effects,
                        });
                    }
                    Err(error) => {
                        fail_safe(machine, hook, normalizer, event_sink, &error.to_string());
                    }
                }
                return true;
            }
            KeyboardHookEvent::Key(event) => event,
        };

        let Some(normalized_event) = normalizer.process(event, &machine.bindings) else {
            return true;
        };

        match machine.handle_key_event(normalized_event) {
            Ok(effects) => {
                hook.set_interception_enabled(machine.enabled());
                if !effects.is_empty() {
                    event_sink.send(RuntimeEvent::Effects {
                        state: machine.snapshot(),
                        effects,
                    });
                }
            }
            Err(error) => {
                fail_safe(machine, hook, normalizer, event_sink, &error.to_string());
            }
        }
        true
    }

    fn apply_command(
        command: RuntimeCommand,
        machine: &mut RuntimeMachine<WindowsPointer>,
        hook: &KeyboardHook,
        normalizer: &mut KeyboardEventNormalizer,
    ) -> Result<(), String> {
        match command {
            RuntimeCommand::Apply(action) => {
                if matches!(
                    action,
                    InputAction::ToggleEnabled | InputAction::SetEnabled(_)
                ) {
                    return Ok(());
                }
                machine
                    .apply_action(action)
                    .map_err(|error| error.to_string())?;
                if !machine.enabled() {
                    machine.motion.stop();
                    normalizer.reset();
                }
                hook.set_interception_enabled(machine.enabled());
            }
            RuntimeCommand::Configure(config) => {
                machine
                    .configure(config)
                    .map_err(|error| error.to_string())?;
                normalizer.reset();
                hook.set_interception_enabled(machine.enabled());
            }
            RuntimeCommand::SetMotionConfig(config) => machine.set_motion_config(config),
            RuntimeCommand::SetBindings(bindings) => {
                machine.set_bindings(bindings);
                normalizer.reset();
            }
            RuntimeCommand::Shutdown => unreachable!("shutdown is handled by the worker loop"),
        }
        Ok(())
    }

    fn apply_num_lock_mode<B: PointerBackend>(
        machine: &mut RuntimeMachine<B>,
        num_lock_on: bool,
    ) -> Result<Vec<CoreEffect>, B::Error> {
        machine.motion.stop();
        machine.apply_action(InputAction::SetEnabled(!num_lock_on))
    }

    fn fail_safe(
        machine: &mut RuntimeMachine<WindowsPointer>,
        hook: &KeyboardHook,
        normalizer: &mut KeyboardEventNormalizer,
        event_sink: &RuntimeEventSink,
        reason: &str,
    ) {
        machine.motion.stop();
        normalizer.reset();
        let effects = machine.controller.shutdown();
        let _ = machine.execute_effects(&effects);
        let release_error = machine.pointer.release_all().err();
        hook.emergency_disable();

        if !effects.is_empty() {
            event_sink.send(RuntimeEvent::Effects {
                state: machine.snapshot(),
                effects,
            });
        }
        let message = release_error.map_or_else(
            || reason.to_owned(),
            |error| format!("{reason}; additionally failed to release pointer state: {error}"),
        );
        event_sink.send(RuntimeEvent::Fault {
            state: machine.snapshot(),
            reason: message,
        });
    }

    #[cfg(test)]
    mod tests {
        use std::{convert::Infallible, time::Duration};

        use numflow_core::{
            Bindings, CoreEffect, Direction, InputAction, MotionConfig, MouseButton, NumpadKey,
            PointerBackend, StateChange,
        };
        use numflow_windows::{KeyState, NormalizedKeyEvent};

        use super::{RuntimeEventSink, RuntimeMachine, apply_num_lock_mode};
        use crate::runtime::{RuntimeConfig, RuntimeEvent, RuntimeStateSnapshot};

        #[derive(Debug, Default)]
        struct MockPointer {
            moves: Vec<(i32, i32)>,
            held: Vec<MouseButton>,
            releases: usize,
            clicks: usize,
            double_clicks: usize,
        }

        impl PointerBackend for MockPointer {
            type Error = Infallible;

            fn move_relative(&mut self, dx: i32, dy: i32) -> Result<(), Self::Error> {
                self.moves.push((dx, dy));
                Ok(())
            }

            fn button_down(&mut self, button: MouseButton) -> Result<(), Self::Error> {
                if !self.held.contains(&button) {
                    self.held.push(button);
                }
                Ok(())
            }

            fn button_up(&mut self, button: MouseButton) -> Result<(), Self::Error> {
                self.held.retain(|held| *held != button);
                self.releases += 1;
                Ok(())
            }

            fn click(&mut self, _button: MouseButton) -> Result<(), Self::Error> {
                self.clicks += 1;
                Ok(())
            }

            fn double_click(&mut self, _button: MouseButton) -> Result<(), Self::Error> {
                self.double_clicks += 1;
                Ok(())
            }

            fn release_all(&mut self) -> Result<(), Self::Error> {
                self.releases += self.held.len();
                self.held.clear();
                Ok(())
            }
        }

        fn runtime_machine() -> RuntimeMachine<MockPointer> {
            RuntimeMachine::new(
                RuntimeConfig::new(
                    MotionConfig::default(),
                    Bindings::default(),
                    MouseButton::Left,
                    false,
                ),
                MockPointer::default(),
            )
        }

        #[test]
        fn runtime_starts_disabled_and_does_not_move_pointer() {
            let mut machine = runtime_machine();
            let event = NormalizedKeyEvent {
                key: NumpadKey::Num8,
                action: InputAction::Move(Direction::Up),
                state: KeyState::Pressed,
                repeated: false,
            };

            machine.handle_key_event(event).expect("mock is infallible");
            machine
                .tick(Duration::from_millis(100))
                .expect("mock is infallible");

            assert!(!machine.enabled());
            assert!(machine.pointer.moves.is_empty());
        }

        #[test]
        fn enabled_runtime_turns_held_move_key_into_pointer_motion() {
            let mut machine = runtime_machine();
            machine
                .apply_action(InputAction::SetEnabled(true))
                .expect("mock is infallible");
            machine
                .handle_key_event(NormalizedKeyEvent {
                    key: NumpadKey::Num8,
                    action: InputAction::Move(Direction::Up),
                    state: KeyState::Pressed,
                    repeated: false,
                })
                .expect("mock is infallible");
            machine
                .tick(Duration::from_millis(100))
                .expect("mock is infallible");

            assert!(!machine.pointer.moves.is_empty());
            assert!(machine.pointer.moves.iter().any(|(_, dy)| *dy < 0));
        }

        #[test]
        fn shutdown_releases_a_runtime_held_button() {
            let mut machine = runtime_machine();
            machine
                .apply_action(InputAction::SetEnabled(true))
                .expect("mock is infallible");
            machine
                .apply_action(InputAction::Hold)
                .expect("mock is infallible");
            assert_eq!(machine.pointer.held, vec![MouseButton::Left]);

            let effects = machine.shutdown().expect("mock is infallible");

            assert!(!machine.enabled());
            assert!(machine.pointer.held.is_empty());
            assert!(!effects.is_empty());
            assert_eq!(machine.pointer.releases, 1);
        }

        #[test]
        fn num_lock_on_disables_runtime_and_releases_drag() {
            let mut machine = runtime_machine();
            apply_num_lock_mode(&mut machine, false).expect("mock is infallible");
            machine
                .apply_action(InputAction::Hold)
                .expect("mock is infallible");
            machine.motion.press(Direction::Right);
            assert!(machine.enabled());
            assert_eq!(machine.pointer.held, vec![MouseButton::Left]);

            let effects = apply_num_lock_mode(&mut machine, true).expect("mock is infallible");

            assert!(!machine.enabled());
            assert!(!machine.motion.is_moving());
            assert!(machine.pointer.held.is_empty());
            assert!(
                effects
                    .iter()
                    .any(|effect| matches!(effect, CoreEffect::State(StateChange::Enabled(false))))
            );
        }

        #[test]
        fn num_lock_off_enables_runtime() {
            let mut machine = runtime_machine();
            let effects = apply_num_lock_mode(&mut machine, false).expect("mock is infallible");

            assert!(machine.enabled());
            assert!(
                effects
                    .iter()
                    .any(|effect| matches!(effect, CoreEffect::State(StateChange::Enabled(true))))
            );
        }

        #[test]
        fn mapped_enable_actions_do_not_override_num_lock_mode() {
            let mut machine = runtime_machine();
            apply_num_lock_mode(&mut machine, false).expect("mock is infallible");

            let effects = machine
                .handle_key_event(NormalizedKeyEvent {
                    key: NumpadKey::Num5,
                    action: InputAction::SetEnabled(false),
                    state: KeyState::Pressed,
                    repeated: false,
                })
                .expect("mock is infallible");

            assert!(effects.is_empty());
            assert!(machine.enabled());
        }

        fn pressed(key: NumpadKey, action: InputAction) -> NormalizedKeyEvent {
            NormalizedKeyEvent {
                key,
                action,
                state: KeyState::Pressed,
                repeated: false,
            }
        }

        #[test]
        fn numpad_zero_holds_selected_button_without_duplicate_mouse_down() {
            for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                let mut machine = runtime_machine();
                apply_num_lock_mode(&mut machine, false).expect("mock is infallible");
                machine
                    .apply_action(InputAction::SelectButton(button))
                    .expect("mock is infallible");

                let first = machine
                    .handle_key_event(pressed(NumpadKey::Num0, InputAction::Hold))
                    .expect("mock is infallible");
                let repeated = machine
                    .handle_key_event(pressed(NumpadKey::Num0, InputAction::Hold))
                    .expect("mock is infallible");

                assert!(!first.is_empty());
                assert!(repeated.is_empty());
                assert_eq!(machine.pointer.held, vec![button]);
                assert_eq!(machine.controller.held_button(), Some(button));
                assert_eq!(machine.controller.selected_button(), button);
            }
        }

        #[test]
        fn five_add_and_decimal_release_each_supported_button_and_reset_state() {
            let release_keys = [
                (NumpadKey::Num5, InputAction::Click),
                (NumpadKey::Add, InputAction::DoubleClick),
                (NumpadKey::Decimal, InputAction::Release),
            ];

            for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                for (release_key, release_action) in release_keys {
                    let mut machine = runtime_machine();
                    apply_num_lock_mode(&mut machine, false).expect("mock is infallible");
                    machine
                        .apply_action(InputAction::SelectButton(button))
                        .expect("mock is infallible");
                    machine
                        .handle_key_event(pressed(NumpadKey::Num0, InputAction::Hold))
                        .expect("mock is infallible");

                    machine
                        .handle_key_event(pressed(release_key, release_action))
                        .expect("mock is infallible");

                    assert!(machine.pointer.held.is_empty());
                    assert_eq!(machine.controller.held_button(), None);
                    assert_eq!(machine.pointer.releases, 1);
                    assert_eq!(machine.pointer.clicks, 0);
                    assert_eq!(machine.pointer.double_clicks, 0);

                    machine
                        .handle_key_event(pressed(NumpadKey::Num0, InputAction::Hold))
                        .expect("mock is infallible");
                    assert_eq!(machine.pointer.held, vec![button]);
                    assert_eq!(machine.controller.held_button(), Some(button));
                }
            }
        }

        #[test]
        fn release_key_sweeps_multiple_backend_tracked_buttons() {
            let mut machine = runtime_machine();
            apply_num_lock_mode(&mut machine, false).expect("mock is infallible");

            // Simulate stale/legacy backend state that contains more buttons than the current
            // single-latch controller model can normally create. The release command must still
            // physically clear every backend-tracked button.
            machine
                .pointer
                .button_down(MouseButton::Left)
                .expect("mock is infallible");
            machine
                .pointer
                .button_down(MouseButton::Right)
                .expect("mock is infallible");
            assert_eq!(
                machine.pointer.held,
                vec![MouseButton::Left, MouseButton::Right]
            );
            assert_eq!(machine.controller.held_button(), None);

            machine
                .handle_key_event(pressed(NumpadKey::Decimal, InputAction::Release))
                .expect("mock is infallible");

            assert!(machine.pointer.held.is_empty());
            assert_eq!(machine.pointer.releases, 2);
            assert_eq!(machine.controller.held_button(), None);
        }

        #[test]
        fn five_and_add_keep_normal_click_behavior_without_hold() {
            let mut machine = runtime_machine();
            apply_num_lock_mode(&mut machine, false).expect("mock is infallible");

            machine
                .handle_key_event(pressed(NumpadKey::Num5, InputAction::Click))
                .expect("mock is infallible");
            machine
                .handle_key_event(pressed(NumpadKey::Add, InputAction::DoubleClick))
                .expect("mock is infallible");
            machine
                .handle_key_event(pressed(NumpadKey::Decimal, InputAction::Release))
                .expect("mock is infallible");

            assert_eq!(machine.pointer.clicks, 1);
            assert_eq!(machine.pointer.double_clicks, 1);
            assert_eq!(machine.controller.held_button(), None);
        }

        #[test]
        fn changing_bindings_stops_existing_motion() {
            let mut machine = runtime_machine();
            machine
                .apply_action(InputAction::SetEnabled(true))
                .expect("mock is infallible");
            machine.motion.press(Direction::Right);
            assert!(machine.motion.is_moving());

            let mut bindings = Bindings::default();
            bindings.bind(NumpadKey::Num6, InputAction::Click);
            machine.set_bindings(bindings);

            assert!(!machine.motion.is_moving());
        }

        fn event_state(button: MouseButton) -> RuntimeStateSnapshot {
            RuntimeStateSnapshot {
                enabled: true,
                selected_button: button,
                held_button: None,
                precision: false,
            }
        }

        #[test]
        fn bounded_event_sink_evicts_oldest_and_keeps_latest_state() {
            let (sender, receiver) = crossbeam_channel::bounded(2);
            let (wake_sender, wake_receiver) = std::sync::mpsc::sync_channel(1);
            let sink = RuntimeEventSink {
                events: sender,
                overflow_reader: receiver.clone(),
                wake: wake_sender,
            };

            for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                sink.send(RuntimeEvent::Effects {
                    state: event_state(button),
                    effects: Vec::new(),
                });
            }

            let events = receiver.try_iter().collect::<Vec<_>>();
            assert_eq!(events.len(), 2);
            assert!(matches!(
                &events[0],
                RuntimeEvent::Effects { state, .. }
                    if state.selected_button == MouseButton::Right
            ));
            assert!(matches!(
                &events[1],
                RuntimeEvent::Effects { state, .. }
                    if state.selected_button == MouseButton::Middle
            ));
            assert!(wake_receiver.try_recv().is_ok());
            assert!(wake_receiver.try_recv().is_err());
        }

        #[test]
        fn fault_is_retained_when_event_queue_is_full() {
            let (sender, receiver) = crossbeam_channel::bounded(2);
            let (wake_sender, _wake_receiver) = std::sync::mpsc::sync_channel(1);
            let sink = RuntimeEventSink {
                events: sender,
                overflow_reader: receiver.clone(),
                wake: wake_sender,
            };

            sink.send(RuntimeEvent::Effects {
                state: event_state(MouseButton::Left),
                effects: Vec::new(),
            });
            sink.send(RuntimeEvent::Effects {
                state: event_state(MouseButton::Right),
                effects: Vec::new(),
            });
            sink.send(RuntimeEvent::Fault {
                state: RuntimeStateSnapshot {
                    enabled: false,
                    selected_button: MouseButton::Right,
                    held_button: None,
                    precision: false,
                },
                reason: "test fault".to_owned(),
            });

            let events = receiver.try_iter().collect::<Vec<_>>();
            assert_eq!(events.len(), 2);
            assert!(matches!(
                &events[1],
                RuntimeEvent::Fault { state, reason }
                    if !state.enabled && reason == "test fault"
            ));
        }
    }
}

#[cfg(windows)]
pub use platform::BackgroundRuntime;

#[cfg(not(windows))]
#[derive(Debug, Default)]
pub struct BackgroundRuntime;

#[cfg(not(windows))]
impl BackgroundRuntime {
    pub fn start(_config: RuntimeConfig) -> Result<Self, RuntimeError> {
        Ok(Self)
    }

    pub fn apply(&self, _action: InputAction) -> Result<(), RuntimeError> {
        Ok(())
    }

    pub fn configure(&self, _config: RuntimeConfig) -> Result<(), RuntimeError> {
        Ok(())
    }

    pub fn set_motion_config(&self, _config: MotionConfig) -> Result<(), RuntimeError> {
        Ok(())
    }

    pub fn set_bindings(&self, _bindings: Bindings) -> Result<(), RuntimeError> {
        Ok(())
    }

    #[must_use]
    pub fn drain_events(&self) -> Vec<RuntimeEvent> {
        Vec::new()
    }

    #[must_use]
    pub fn take_wake_receiver(&mut self) -> Option<std::sync::mpsc::Receiver<()>> {
        None
    }

    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
