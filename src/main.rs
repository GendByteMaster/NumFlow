mod app;
mod bindings_ui;
mod config;
mod error;
mod hud;
mod runtime;

slint::include_modules!();

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("numflow=info"));

    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn main() {
    init_tracing();

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

    let tray = match AppTray::new() {
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
        tracing::error!(%error, "NumFlow terminated with an error");
        std::process::exit(1);
    }
}
