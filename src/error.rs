#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("UI error: {0}")]
    Ui(String),
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
}
