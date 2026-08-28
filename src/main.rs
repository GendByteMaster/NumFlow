mod app;
mod bindings_ui;
mod config;
mod error;
mod hud;
mod platform_input;
mod runtime;

slint::include_modules!();

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("numflow=info"));

    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn main() {
    init_tracing();

    let background = std::env::args_os()
        .skip(1)
        .any(|argument| argument == std::ffi::OsStr::new("--background"));
    #[cfg(windows)]
    let elevated = std::env::args_os()
        .skip(1)
        .any(|argument| argument == std::ffi::OsStr::new("--elevated"));

    #[cfg(windows)]
    if elevated && numflow_windows::current_process_elevated() != Some(true) {
        match numflow_windows::relaunch_elevated(background) {
            Ok(()) => return,
            Err(error) => {
                tracing::error!(%error, "failed to launch the explicit elevated NumFlow profile");
                std::process::exit(1);
            }
        }
    }

    #[cfg(windows)]
    let _instance_guard = match numflow_windows::SingleInstanceGuard::acquire() {
        Ok(guard) => guard,
        Err(numflow_windows::SingleInstanceError::AlreadyRunning) => {
            tracing::info!("another NumFlow instance is already running; exiting");
            return;
        }
        Err(error) => {
            tracing::error!(%error, "failed to acquire NumFlow single-instance guard");
            std::process::exit(1);
        }
    };

    #[cfg(windows)]
    let _at_session = match numflow_windows::AssistiveTechnologySession::start() {
        Ok(session) => Some(session),
        Err(error) => {
            tracing::warn!(%error, "failed to notify Ease of Access about the NumFlow session");
            None
        }
    };

    if let Err(error) = app::run(background) {
        tracing::error!(%error, "NumFlow terminated with an error");
        std::process::exit(1);
    }
}
