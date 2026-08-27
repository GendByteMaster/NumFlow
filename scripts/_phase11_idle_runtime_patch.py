from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"Expected exactly one {label} marker, found {count}")
    return text.replace(old, new, 1)


root_cargo_path = Path("Cargo.toml")
root_cargo = root_cargo_path.read_text(encoding="utf-8")
root_cargo = replace_once(
    root_cargo,
    "[target.'cfg(windows)'.dependencies]\nnumflow-windows = { path = \"crates/numflow-windows\" }",
    "[target.'cfg(windows)'.dependencies]\ncrossbeam-channel = \"0.5\"\nnumflow-windows = { path = \"crates/numflow-windows\" }",
    "root Windows dependency section",
)
root_cargo_path.write_text(root_cargo, encoding="utf-8")

windows_cargo_path = Path("crates/numflow-windows/Cargo.toml")
windows_cargo = windows_cargo_path.read_text(encoding="utf-8")
windows_cargo = replace_once(
    windows_cargo,
    "[target.'cfg(windows)'.dependencies]\nwindows = { version = \"0.62.2\", features = [",
    "[target.'cfg(windows)'.dependencies]\ncrossbeam-channel = \"0.5\"\nwindows = { version = \"0.62.2\", features = [",
    "Windows backend dependency section",
)
windows_cargo_path.write_text(windows_cargo, encoding="utf-8")

hook_path = Path("crates/numflow-windows/src/hook.rs")
hook = hook_path.read_text(encoding="utf-8")
hook = replace_once(
    hook,
    "        mpsc::{self, Receiver, SyncSender, TrySendError},",
    "        mpsc::{self, SyncSender},",
    "hook std mpsc import",
)
hook = replace_once(
    hook,
    "use windows::{",
    "use crossbeam_channel::{Receiver, Sender, TrySendError};\nuse windows::{",
    "crossbeam hook import",
)
hook = replace_once(
    hook,
    "static EVENT_SENDER: OnceLock<Mutex<Option<SyncSender<PhysicalKeyEvent>>>> = OnceLock::new();",
    "static EVENT_SENDER: OnceLock<Mutex<Option<Sender<PhysicalKeyEvent>>>> = OnceLock::new();",
    "hook event sender static",
)
hook = replace_once(
    hook,
    "        let (event_sender, event_receiver) = mpsc::sync_channel(capacity);",
    "        let (event_sender, event_receiver) = crossbeam_channel::bounded(capacity);",
    "hook bounded event channel",
)
hook = replace_once(
    hook,
    "    event_sender: SyncSender<PhysicalKeyEvent>,",
    "    event_sender: Sender<PhysicalKeyEvent>,",
    "hook thread sender type",
)
hook = replace_once(
    hook,
    "fn register_sender(sender: SyncSender<PhysicalKeyEvent>) -> bool {",
    "fn register_sender(sender: Sender<PhysicalKeyEvent>) -> bool {",
    "hook register sender type",
)
hook_path.write_text(hook, encoding="utf-8")

runtime_path = Path("src/runtime.rs")
runtime = runtime_path.read_text(encoding="utf-8")
runtime = replace_once(
    runtime,
    "    #[error(\"NumFlow background runtime command channel is closed\")]\n    CommandChannelClosed,",
    "    #[error(\"NumFlow background runtime command channel is closed\")]\n    CommandChannelClosed,\n    #[error(\"NumFlow background runtime command queue is full\")]\n    CommandQueueFull,",
    "runtime queue-full error",
)
runtime = replace_once(
    runtime,
    "        sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError},",
    "        sync::mpsc::{self, Receiver as StdReceiver, Sender as StdSender, SyncSender},",
    "runtime std mpsc import",
)
runtime = replace_once(
    runtime,
    "    use numflow_core::{",
    "    use crossbeam_channel::{Receiver, Sender, TrySendError};\n    use numflow_core::{",
    "runtime crossbeam import",
)
runtime = replace_once(
    runtime,
    "        KeyState, KeyboardEventNormalizer, KeyboardHook, NormalizedKeyEvent, WindowsPointer,",
    "        KeyState, KeyboardEventNormalizer, KeyboardHook, NormalizedKeyEvent, PhysicalKeyEvent,\n        WindowsPointer,",
    "runtime physical key import",
)
runtime = replace_once(
    runtime,
    "    const MOTION_TICK: Duration = Duration::from_millis(8);",
    "    const MOTION_TICK: Duration = Duration::from_millis(8);\n    const COMMAND_QUEUE_CAPACITY: usize = 64;",
    "runtime constants",
)
runtime = replace_once(
    runtime,
    "        events: Sender<RuntimeEvent>,",
    "        events: StdSender<RuntimeEvent>,",
    "runtime event sink sender",
)
runtime = replace_once(
    runtime,
    "        event_receiver: Receiver<RuntimeEvent>,\n        wake_receiver: Option<Receiver<()>>,",
    "        event_receiver: StdReceiver<RuntimeEvent>,\n        wake_receiver: Option<StdReceiver<()>>,",
    "runtime std event receivers",
)
runtime = replace_once(
    runtime,
    "            let (command_sender, command_receiver) = mpsc::channel();",
    "            let (command_sender, command_receiver) =\n                crossbeam_channel::bounded(COMMAND_QUEUE_CAPACITY);",
    "runtime bounded command channel",
)
runtime = replace_once(
    runtime,
    "        pub fn take_wake_receiver(&mut self) -> Option<Receiver<()>> {",
    "        pub fn take_wake_receiver(&mut self) -> Option<StdReceiver<()>> {",
    "runtime wake receiver type",
)
runtime = replace_once(
    runtime,
    "        fn send(&self, command: RuntimeCommand) -> Result<(), RuntimeError> {\n            self.command_sender\n                .send(command)\n                .map_err(|_| RuntimeError::CommandChannelClosed)\n        }",
    "        fn send(&self, command: RuntimeCommand) -> Result<(), RuntimeError> {\n            match self.command_sender.try_send(command) {\n                Ok(()) => Ok(()),\n                Err(TrySendError::Full(_)) => Err(RuntimeError::CommandQueueFull),\n                Err(TrySendError::Disconnected(_)) => Err(RuntimeError::CommandChannelClosed),\n            }\n        }",
    "runtime non-blocking command send",
)
old_loop = '''        let mut previous_tick = Instant::now();
        let mut running = true;

        while running {
            let loop_started = Instant::now();

            loop {
                match command_receiver.try_recv() {
                    Ok(RuntimeCommand::Shutdown) => {
                        let effects = match machine.shutdown() {
                            Ok(effects) => effects,
                            Err(error) => {
                                event_sink.send(RuntimeEvent::Fault(error.to_string()));
                                Vec::new()
                            }
                        };
                        if !effects.is_empty() {
                            event_sink.send(RuntimeEvent::Effects(effects));
                        }
                        hook.emergency_disable();
                        running = false;
                        break;
                    }
                    Ok(command) => {
                        if let Err(error) =
                            apply_command(command, &mut machine, &hook, &mut normalizer)
                        {
                            fail_safe(&mut machine, &hook, &mut normalizer, event_sink, &error);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        let _ = machine.shutdown();
                        hook.emergency_disable();
                        running = false;
                        break;
                    }
                }
            }

            if !running {
                break;
            }

            while let Ok(event) = keyboard_receiver.try_recv() {
                let Some(normalized_event) = normalizer.process(event, &machine.bindings) else {
                    continue;
                };
                match machine.handle_key_event(normalized_event) {
                    Ok(effects) => {
                        if effects.iter().any(|effect| {
                            matches!(effect, CoreEffect::State(StateChange::Enabled(false)))
                        }) {
                            normalizer.reset();
                        }
                        hook.set_interception_enabled(machine.enabled());
                        if !effects.is_empty() {
                            event_sink.send(RuntimeEvent::Effects(effects));
                        }
                    }
                    Err(error) => {
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

            let spent = loop_started.elapsed();
            if spent < MOTION_TICK {
                thread::sleep(MOTION_TICK.checked_sub(spent).unwrap());
            }
        }
'''
new_loop = '''        let motion_tick = crossbeam_channel::tick(MOTION_TICK);
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
                        );
                    }
                }
                previous_tick = Instant::now();
            }
        }
'''
runtime = replace_once(runtime, old_loop, new_loop, "runtime polling loop")
helper_marker = "    fn apply_command(\n"
helpers = '''    fn handle_command_message(
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
                        event_sink.send(RuntimeEvent::Fault(error.to_string()));
                        Vec::new()
                    }
                };
                if !effects.is_empty() {
                    event_sink.send(RuntimeEvent::Effects(effects));
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
        event: Result<PhysicalKeyEvent, crossbeam_channel::RecvError>,
        machine: &mut RuntimeMachine<WindowsPointer>,
        hook: &KeyboardHook,
        normalizer: &mut KeyboardEventNormalizer,
        event_sink: &RuntimeEventSink,
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
        let Some(normalized_event) = normalizer.process(event, &machine.bindings) else {
            return true;
        };

        match machine.handle_key_event(normalized_event) {
            Ok(effects) => {
                if effects
                    .iter()
                    .any(|effect| matches!(effect, CoreEffect::State(StateChange::Enabled(false))))
                {
                    normalizer.reset();
                }
                hook.set_interception_enabled(machine.enabled());
                if !effects.is_empty() {
                    event_sink.send(RuntimeEvent::Effects(effects));
                }
            }
            Err(error) => {
                fail_safe(
                    machine,
                    hook,
                    normalizer,
                    event_sink,
                    &error.to_string(),
                );
            }
        }
        true
    }

'''
runtime = replace_once(runtime, helper_marker, helpers + helper_marker, "runtime event helpers")
runtime_path.write_text(runtime, encoding="utf-8")
