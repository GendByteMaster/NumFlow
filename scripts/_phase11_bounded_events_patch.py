from pathlib import Path
import re


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one marker, found {count}: {old[:80]!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


runtime = Path("src/runtime.rs")
app = Path("src/app.rs")

replace_once(
    runtime,
    "#[derive(Debug, Clone, PartialEq)]\npub enum RuntimeEvent {\n    Effects(Vec<CoreEffect>),\n    Fault(String),\n}\n",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub struct RuntimeStateSnapshot {\n    pub enabled: bool,\n    pub selected_button: MouseButton,\n    pub held_button: Option<MouseButton>,\n    pub precision: bool,\n}\n\n#[derive(Debug, Clone, PartialEq)]\npub enum RuntimeEvent {\n    Effects {\n        state: RuntimeStateSnapshot,\n        effects: Vec<CoreEffect>,\n    },\n    Fault {\n        state: RuntimeStateSnapshot,\n        reason: String,\n    },\n}\n",
)

replace_once(
    runtime,
    "sync::mpsc::{self, Receiver as StdReceiver, Sender as StdSender, SyncSender},",
    "sync::mpsc::{self, Receiver as StdReceiver, SyncSender},",
)
replace_once(
    runtime,
    "use super::{RuntimeConfig, RuntimeError, RuntimeEvent};",
    "use super::{RuntimeConfig, RuntimeError, RuntimeEvent, RuntimeStateSnapshot};",
)
replace_once(
    runtime,
    "const COMMAND_QUEUE_CAPACITY: usize = 64;",
    "const COMMAND_QUEUE_CAPACITY: usize = 64;\n    const EVENT_QUEUE_CAPACITY: usize = 64;",
)

old_sink = '''    #[derive(Debug)]
    struct RuntimeEventSink {
        events: StdSender<RuntimeEvent>,
        wake: SyncSender<()>,
    }

    impl RuntimeEventSink {
        fn send(&self, event: RuntimeEvent) {
            if self.events.send(event).is_ok() {
                let _ = self.wake.try_send(());
            }
        }
    }
'''
new_sink = '''    #[derive(Debug)]
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
'''
replace_once(runtime, old_sink, new_sink)
replace_once(runtime, "event_receiver: StdReceiver<RuntimeEvent>,", "event_receiver: Receiver<RuntimeEvent>,")
replace_once(
    runtime,
    "            let (event_sink, event_receiver) = mpsc::channel();\n            let (wake_sender, wake_receiver) = mpsc::sync_channel(1);",
    "            let (event_sender, event_receiver) =\n                crossbeam_channel::bounded(EVENT_QUEUE_CAPACITY);\n            let event_overflow_reader = event_receiver.clone();\n            let (wake_sender, wake_receiver) = mpsc::sync_channel(1);",
)
replace_once(
    runtime,
    "                    let event_sink = RuntimeEventSink {\n                        events: event_sink,\n                        wake: wake_sender,\n                    };",
    "                    let event_sink = RuntimeEventSink {\n                        events: event_sender,\n                        overflow_reader: event_overflow_reader,\n                        wake: wake_sender,\n                    };",
)

replace_once(
    runtime,
    "        fn enabled(&self) -> bool {\n            self.controller.is_enabled()\n        }",
    "        fn enabled(&self) -> bool {\n            self.controller.is_enabled()\n        }\n\n        fn snapshot(&self) -> RuntimeStateSnapshot {\n            RuntimeStateSnapshot {\n                enabled: self.controller.is_enabled(),\n                selected_button: self.controller.selected_button(),\n                held_button: self.controller.held_button(),\n                precision: self.controller.is_precision_enabled(),\n            }\n        }",
)

text = runtime.read_text(encoding="utf-8")
old_effect = "event_sink.send(RuntimeEvent::Effects(effects));"
if text.count(old_effect) != 3:
    raise SystemExit(f"runtime: expected 3 effect sends, found {text.count(old_effect)}")
text = text.replace(
    old_effect,
    "event_sink.send(RuntimeEvent::Effects {\n                        state: machine.snapshot(),\n                        effects,\n                    });",
)
text = text.replace(
    "event_sink.send(RuntimeEvent::Fault(error.to_string()));",
    "event_sink.send(RuntimeEvent::Fault {\n                            state: machine.snapshot(),\n                            reason: error.to_string(),\n                        });",
    1,
)
text = text.replace(
    "event_sink.send(RuntimeEvent::Fault(message));",
    "event_sink.send(RuntimeEvent::Fault {\n            state: machine.snapshot(),\n            reason: message,\n        });",
    1,
)
runtime.write_text(text, encoding="utf-8")

# Add bounded mailbox regression tests next to the runtime machine tests.
text = runtime.read_text(encoding="utf-8")
needle = '''        #[test]
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
'''
addition = needle + '''
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
'''
if text.count(needle) != 1:
    raise SystemExit("runtime: changing_bindings test marker not unique")
text = text.replace(needle, addition, 1)
# Extend test imports for sink/snapshot/event types.
text = text.replace(
    "        use super::RuntimeMachine;\n        use crate::runtime::RuntimeConfig;",
    "        use super::{RuntimeEventSink, RuntimeMachine};\n        use crate::runtime::{RuntimeConfig, RuntimeEvent, RuntimeStateSnapshot};",
    1,
)
runtime.write_text(text, encoding="utf-8")

# UI: import snapshot, replace incremental replay with authoritative state synchronization.
replace_once(
    app,
    "    runtime::{BackgroundRuntime, RuntimeConfig, RuntimeEvent},",
    "    runtime::{BackgroundRuntime, RuntimeConfig, RuntimeEvent, RuntimeStateSnapshot},",
)
text = app.read_text(encoding="utf-8")
pattern = re.compile(r"fn apply_runtime_effects\(settings: &mut UiSettings, effects: &\[CoreEffect\]\) -> bool \{.*?\n\}\n\nfn start_runtime_event_bridge", re.S)
replacement = '''fn sync_runtime_state(settings: &mut UiSettings, state: RuntimeStateSnapshot) -> bool {
    let selected_button = state.selected_button.into();
    let profile = settings
        .config
        .profiles
        .get_mut(&settings.config.active_profile)
        .expect("active profile must exist");
    let config_changed =
        profile.selected_button != selected_button || profile.precision_enabled != state.precision;
    profile.selected_button = selected_button;
    profile.precision_enabled = state.precision;

    // Rebuild the UI-side state machine from the authoritative runtime snapshot. This keeps
    // settings/tray/HUD state correct even if an older transient event was evicted under load.
    let _ = settings.controller.apply(InputAction::SetEnabled(false));
    let _ = settings
        .controller
        .apply(InputAction::SelectButton(state.selected_button));
    let _ = settings
        .controller
        .apply(InputAction::SetPrecision(state.precision));

    if state.enabled {
        let _ = settings.controller.apply(InputAction::SetEnabled(true));
        if let Some(held_button) = state.held_button {
            let _ = settings
                .controller
                .apply(InputAction::SelectButton(held_button));
            let _ = settings.controller.apply(InputAction::Hold);
            let _ = settings
                .controller
                .apply(InputAction::SelectButton(state.selected_button));
        }
    }

    config_changed
}

fn start_runtime_event_bridge'''
text, count = pattern.subn(replacement, text, count=1)
if count != 1:
    raise SystemExit(f"app: apply_runtime_effects block replacement count={count}")
old_match = '''                RuntimeEvent::Effects(effects) => {
                    config_changed |= apply_runtime_effects(&mut settings.borrow_mut(), &effects);
                    hud.borrow_mut().observe_effects(&effects);
                    state_changed = true;
                }
                RuntimeEvent::Fault(reason) => {
                    tracing::error!(%reason, "NumFlow background pointer runtime entered safe disabled state");
                    let effects = settings.borrow_mut().set_enabled(false);
                    hud.borrow_mut().observe_effects(&effects);
                    state_changed = true;
                }
'''
new_match = '''                RuntimeEvent::Effects { state, effects } => {
                    config_changed |= sync_runtime_state(&mut settings.borrow_mut(), state);
                    hud.borrow_mut().observe_effects(&effects);
                    state_changed = true;
                }
                RuntimeEvent::Fault { state, reason } => {
                    config_changed |= sync_runtime_state(&mut settings.borrow_mut(), state);
                    tracing::error!(%reason, "NumFlow background pointer runtime entered safe disabled state");
                    hud.borrow_mut().observe_effects(&[CoreEffect::State(
                        StateChange::Enabled(false),
                    )]);
                    state_changed = true;
                }
'''
if text.count(old_match) != 1:
    raise SystemExit(f"app: runtime event match marker count={text.count(old_match)}")
text = text.replace(old_match, new_match, 1)
# Add a UI resynchronization regression test.
text = text.replace(
    "    use super::{DEFAULT_POINTER_ACCELERATION, DEFAULT_POINTER_SPEED, UiSettings};",
    "    use super::{\n        DEFAULT_POINTER_ACCELERATION, DEFAULT_POINTER_SPEED, UiSettings, sync_runtime_state,\n    };",
    1,
)
text = text.replace(
    "    use numflow_core::{Direction, InputAction, MotionConfig, MouseButton, NumpadKey};",
    "    use numflow_core::{Direction, InputAction, MotionConfig, MouseButton, NumpadKey};\n    use crate::runtime::RuntimeStateSnapshot;",
    1,
)
insert_marker = '''    #[test]
    fn ui_defaults_match_core_motion_defaults() {
'''
insert_test = '''    #[test]
    fn runtime_snapshot_resynchronizes_ui_state_after_event_coalescing() {
        let mut settings = UiSettings::default();
        let changed = sync_runtime_state(
            &mut settings,
            RuntimeStateSnapshot {
                enabled: true,
                selected_button: MouseButton::Right,
                held_button: Some(MouseButton::Left),
                precision: true,
            },
        );

        assert!(changed);
        assert!(settings.controller.is_enabled());
        assert_eq!(settings.controller.selected_button(), MouseButton::Right);
        assert_eq!(settings.controller.held_button(), Some(MouseButton::Left));
        assert!(settings.controller.is_precision_enabled());
        assert_eq!(
            MouseButton::from(settings.config.active_profile().selected_button),
            MouseButton::Right
        );
        assert!(settings.config.active_profile().precision_enabled);
    }

'''
if text.count(insert_marker) != 1:
    raise SystemExit("app: test insertion marker not unique")
text = text.replace(insert_marker, insert_test + insert_marker, 1)
app.write_text(text, encoding="utf-8")

# Final assertions: the UI event path must no longer be unbounded.
final_runtime = runtime.read_text(encoding="utf-8")
if "mpsc::channel()" in final_runtime:
    raise SystemExit("runtime still contains an unbounded std mpsc channel")
if "crossbeam_channel::bounded(EVENT_QUEUE_CAPACITY)" not in final_runtime:
    raise SystemExit("bounded runtime event queue was not installed")
print("Phase 11 bounded/coalesced runtime event patch applied")
