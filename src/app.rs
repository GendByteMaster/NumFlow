use slint::ComponentHandle;

use crate::{AppWindow, error::AppError};

pub fn run() -> Result<(), AppError> {
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting NumFlow");

    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;

    window
        .run()
        .map_err(|error| AppError::Ui(error.to_string()))
}
