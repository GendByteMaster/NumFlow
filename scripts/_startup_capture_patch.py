from pathlib import Path

main = Path("src/main.rs")
text = main.read_text(encoding="utf-8")
old = '''    let tray = match AppTray::new() {
        Ok(tray) => {
            tracing::info!("NumFlow system tray ready; closing settings keeps the app running");
            tray
        }
        Err(error) => {
            tracing::error!(%error, "failed to create NumFlow system tray");
            std::process::exit(1);
        }
    };

    if let Err(error) = app::run(&tray) {
'''
new = '''    if let Err(error) = app::run() {
'''
if old not in text:
    raise SystemExit("main.rs startup marker not found")
main.write_text(text.replace(old, new, 1), encoding="utf-8")

app = Path("src/app.rs")
text = app.read_text(encoding="utf-8")
text = text.replace("pub fn run(tray: &AppTray) -> Result<(), AppError> {", "pub fn run() -> Result<(), AppError> {", 1)
old = '''    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;
    let settings = Rc::new(RefCell::new(UiSettings::from_config(loaded.config)));
    sync_window_from_settings(&window, &settings.borrow());
    sync_tray_from_settings(tray, &settings.borrow());

    if !set_windows_startup(settings.borrow().start_with_windows()) {
        tracing::warn!("configured Windows startup preference could not be applied");
    }

    let mut background_runtime = BackgroundRuntime::start(settings.borrow().runtime_config())
        .map_err(|error| AppError::Runtime(error.to_string()))?;
    let runtime_wake_receiver = background_runtime.take_wake_receiver();
    let runtime = Rc::new(RefCell::new(background_runtime));

    let hud = Rc::new(RefCell::new(
'''
new = '''    let settings = Rc::new(RefCell::new(UiSettings::from_config(loaded.config)));

    // Install the low-level keyboard hook and apply the current Num Lock mode before creating
    // any visible NumFlow UI. Once the tray icon appears, keyboard interception is already ready.
    let mut background_runtime = BackgroundRuntime::start(settings.borrow().runtime_config())
        .map_err(|error| AppError::Runtime(error.to_string()))?;
    let runtime_wake_receiver = background_runtime.take_wake_receiver();
    let runtime = Rc::new(RefCell::new(background_runtime));

    let tray = AppTray::new().map_err(|error| AppError::Ui(error.to_string()))?;
    tracing::info!("NumFlow system tray ready; keyboard runtime is already active");
    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;
    sync_window_from_settings(&window, &settings.borrow());
    sync_tray_from_settings(&tray, &settings.borrow());

    if !set_windows_startup(settings.borrow().start_with_windows()) {
        tracing::warn!("configured Windows startup preference could not be applied");
    }

    let hud = Rc::new(RefCell::new(
'''
if old not in text:
    raise SystemExit("app.rs startup block marker not found")
text = text.replace(old, new, 1)
text = text.replace("    connect_ui(&window, tray, &settings, &hud, &store, &runtime);", "    connect_ui(&window, &tray, &settings, &hud, &store, &runtime);", 1)
text = text.replace("        tray,\n        &settings,", "        &tray,\n        &settings,", 1)
app.write_text(text, encoding="utf-8")
