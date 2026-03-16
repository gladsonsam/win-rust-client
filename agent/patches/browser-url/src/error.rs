use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrowserInfoError {
    #[error("No active window found")]
    WindowNotFound,

    #[error("Active window is not a browser")]
    NotABrowser,

    #[error("Failed to extract URL from browser: {0}")]
    UrlExtractionFailed(String),

    #[error("Browser detection failed: {0}")]
    BrowserDetectionFailed(String),

    #[error("Platform-specific error: {0}")]
    PlatformError(String),

    #[error("Invalid URL format: {0}")]
    InvalidUrl(String),

    #[error("Timeout during operation")]
    Timeout,

    #[error("Other error: {0}")]
    Other(String),
}

pub type BrowserError = BrowserInfoError;
