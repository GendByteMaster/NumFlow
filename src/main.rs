mod app;
mod error;
mod hud;

slint::include_modules!();

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("numflow=info"));

    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn main() {
    init_tracing();

    if let Err(error) = app::run() {
        tracing::error!(%error, "NumFlow terminated with an error");
        std::process::exit(1);
    }
}
