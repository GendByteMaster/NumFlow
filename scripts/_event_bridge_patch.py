from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if text.count(old) != 1:
        raise SystemExit(f"Expected exactly one {label} marker, found {text.count(old)}")
    return text.replace(old, new, 1)


runtime_path = Path("src/runtime.rs")
runtime = runtime_path.read_text(encoding="utf-8")

runtime = replace_once(
    runtime,
    "sync::mpsc::{self, Receiver, Sender, TryRecvError},",
    "sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError},",
    "runtime mpsc import",
)

runtime = replace_once(
    runtime,
    """    #[derive(Debug)]
    pub struct BackgroundRuntime {
""",
    """    #[derive(Debug)]
    struct RuntimeEventSink {
        events: Sender<RuntimeEvent>,
        wake: SyncSender<()>,
    }

    impl RuntimeEventSink {
        fn send(&self, event: RuntimeEvent) {
            if self.events.send(event).is_ok() {
                let _ = self.wake.try_send(());
            }
        }
    }

    #[derive(Debug)]
    pub struct BackgroundRuntime {
""",
    "runtime event sink insertion",
)

runtime = replace_once(
    runtime,
    """        event_receiver: Receiver<RuntimeEvent>,
        join: Option<JoinHandle<()>>,
""",
    """        event_receiver: Receiver<RuntimeEvent>,
        wake_receiver: Option<Receiver<()>>,
        join: Option<JoinHandle<()>>,
""",
    "runtime wake receiver field",
)

runtime = replace_once(
    runtime,
    """            let (event_sender, event_receiver) = mpsc::channel();
            let (ready_sender, ready_receiver) = mpsc::sync_channel(1);

            let join = thread::Builder::new()
                .name("numflow-runtime".to_owned())
                .spawn(move || {
                    worker_main(config, &command_receiver, &event_sender, &ready_sender);
                })
""",
    """            let (event_sender, event_receiver) = mpsc::channel();
            let (wake_sender, wake_receiver) = mpsc::sync_channel(1);
            let (ready_sender, ready_receiver) = mpsc::sync_channel(1);

            let join = thread::Builder::new()
                .name("numflow-runtime".to_owned())
                .spawn(move || {
                    let event_sink = RuntimeEventSink {
                        events: event_sender,
                        wake: wake_sender,
                    };
                    worker_main(config, &command_receiver, &event_sink, &ready_sender);
                })
""",
    "runtime startup channels",
)

runtime = replace_once(
    runtime,
    """                    command_sender,
                    event_receiver,
                    join: Some(join),
""",
    """                    command_sender,
                    event_receiver,
                    wake_receiver: Some(wake_receiver),
                    join: Some(join),
""",
    "runtime construction",
)

runtime = replace_once(
    runtime,
    """        #[must_use]
        pub fn drain_events(&self) -> Vec<RuntimeEvent> {
            let mut events = Vec::new();
            while let Ok(event) = self.event_receiver.try_recv() {
                events.push(event);
            }
            events
        }

        pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
""",
    """        #[must_use]
        pub fn drain_events(&self) -> Vec<RuntimeEvent> {
            let mut events = Vec::new();
            while let Ok(event) = self.event_receiver.try_recv() {
                events.push(event);
            }
            events
        }

        #[must_use]
        pub fn take_wake_receiver(&mut self) -> Option<Receiver<()>> {
            self.wake_receiver.take()
        }

        pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
""",
    "runtime wake receiver accessor",
)

runtime = runtime.replace(
    "event_sender: &Sender<RuntimeEvent>,",
    "event_sink: &RuntimeEventSink,",
)
runtime = runtime.replace("event_sender,", "event_sink,")
runtime = runtime.replace("event_sender.send(", "event_sink.send(")
runtime = runtime.replace("let _ = event_sink.send(", "event_sink.send(")

runtime = replace_once(
    runtime,
    """    #[must_use]
    pub fn drain_events(&self) -> Vec<RuntimeEvent> {
        Vec::new()
    }

    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
""",
    """    #[must_use]
    pub fn drain_events(&self) -> Vec<RuntimeEvent> {
        Vec::new()
    }

    #[must_use]
    pub fn take_wake_receiver(&mut self) -> Option<std::sync::mpsc::Receiver<()>> {
        None
    }

    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
""",
    "non-Windows wake receiver accessor",
)

runtime_path.write_text(runtime, encoding="utf-8")

app_path = Path("src/app.rs")
app = app_path.read_text(encoding="utf-8")

app = replace_once(
    app,
    "use std::{cell::RefCell, rc::Rc, time::Duration};",
    """use std::{
    cell::RefCell,
    rc::Rc,
    sync::mpsc::Receiver,
    thread::{self, JoinHandle},
};""",
    "app std imports",
)
app = replace_once(
    app,
    "use slint::{ComponentHandle, Timer, TimerMode};",
    "use slint::ComponentHandle;",
    "Slint timer import",
)
app = replace_once(
    app,
    "const RUNTIME_EVENT_POLL: Duration = Duration::from_millis(16);\n",
    "",
    "runtime poll constant",
)

bridge_start = app.index("fn start_runtime_event_bridge(")
bridge_end = app.index("\nfn connect_ui(", bridge_start)
new_bridge = '''fn start_runtime_event_bridge(
    window: &AppWindow,
    tray: &AppTray,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
    runtime: &SharedRuntime,
    wake_receiver: Option<Receiver<()>>,
) -> Result<Option<JoinHandle<()>>, AppError> {
    let weak_window = window.as_weak();
    let weak_tray = tray.as_weak();
    let settings = Rc::clone(settings);
    let hud = Rc::clone(hud);
    let store = Rc::clone(store);
    let runtime = Rc::clone(runtime);

    window.on_runtime_events_ready(move || {
        let events = runtime.borrow().drain_events();
        if events.is_empty() {
            return;
        }

        let mut state_changed = false;
        let mut config_changed = false;
        for event in events {
            match event {
                RuntimeEvent::Effects(effects) => {
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
            }
        }

        if state_changed {
            if let Some(window) = weak_window.upgrade() {
                sync_window_from_settings(&window, &settings.borrow());
            }
            if let Some(tray) = weak_tray.upgrade() {
                sync_tray_from_settings(&tray, &settings.borrow());
            }
        }
        if config_changed {
            persist_configuration(&settings, &store);
        }
    });

    let Some(wake_receiver) = wake_receiver else {
        return Ok(None);
    };
    let weak_window = window.as_weak();
    let join = thread::Builder::new()
        .name("numflow-runtime-events".to_owned())
        .spawn(move || {
            while wake_receiver.recv().is_ok() {
                if weak_window
                    .upgrade_in_event_loop(|window| window.invoke_runtime_events_ready())
                    .is_err()
                {
                    break;
                }
            }
        })
        .map_err(|error| AppError::Runtime(format!("failed to start runtime event bridge: {error}")))?;

    Ok(Some(join))
}
'''
app = app[:bridge_start] + new_bridge + app[bridge_end:]

app = replace_once(
    app,
    '''    let runtime = Rc::new(RefCell::new(
        BackgroundRuntime::start(settings.borrow().runtime_config())
            .map_err(|error| AppError::Runtime(error.to_string()))?,
    ));
''',
    '''    let mut background_runtime = BackgroundRuntime::start(settings.borrow().runtime_config())
        .map_err(|error| AppError::Runtime(error.to_string()))?;
    let runtime_wake_receiver = background_runtime.take_wake_receiver();
    let runtime = Rc::new(RefCell::new(background_runtime));
''',
    "runtime construction in app",
)

app = replace_once(
    app,
    '''    connect_ui(&window, tray, &settings, &hud, &store, &runtime);
    let _runtime_event_timer =
        start_runtime_event_bridge(&window, tray, &settings, &hud, &store, &runtime);
''',
    '''    connect_ui(&window, tray, &settings, &hud, &store, &runtime);
    let runtime_event_bridge = start_runtime_event_bridge(
        &window,
        tray,
        &settings,
        &hud,
        &store,
        &runtime,
        runtime_wake_receiver,
    )?;
''',
    "runtime bridge startup",
)

app = replace_once(
    app,
    '''    if let Err(error) = runtime.borrow_mut().shutdown() {
        tracing::error!(%error, "background runtime failed during final shutdown");
    }
    event_loop_result
''',
    '''    if let Err(error) = runtime.borrow_mut().shutdown() {
        tracing::error!(%error, "background runtime failed during final shutdown");
    }
    if let Some(join) = runtime_event_bridge
        && join.join().is_err()
    {
        tracing::error!("runtime event bridge thread panicked during shutdown");
    }
    event_loop_result
''',
    "runtime bridge shutdown",
)

app_path.write_text(app, encoding="utf-8")

ui_path = Path("ui/app.slint")
ui = ui_path.read_text(encoding="utf-8")
ui = replace_once(
    ui,
    "    callback reset-defaults();\n",
    "    callback reset-defaults();\n    callback runtime-events-ready();\n",
    "runtime events callback",
)
ui_path.write_text(ui, encoding="utf-8")
