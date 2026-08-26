#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("UI error: {0}")]
    Ui(String),
    #[error("background runtime error: {0}")]
    Runtime(String),
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
}
