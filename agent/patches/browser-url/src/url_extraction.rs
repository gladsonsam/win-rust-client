use crate::{BrowserInfoError, BrowserType};
use active_win_pos_rs::ActiveWindow;

pub fn extract_url(
    window: &ActiveWindow,
    browser_type: &BrowserType,
) -> Result<String, BrowserInfoError> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::extract_url(window, browser_type)
    }

    #[cfg(target_os = "macos")]
    {
        let _ = (window, browser_type); // Suppress unused variable warnings
        // TODO: Implement Macos URL extraction
        Err(BrowserInfoError::PlatformError(
            "Macos not yet implemented".to_string(),
        ))
    }

    #[cfg(target_os = "linux")]
    {
        let _ = (window, browser_type); // Suppress unused variable warnings
        // TODO: Implement Linux URL extraction
        Err(BrowserInfoError::PlatformError(
            "Linux not yet implemented".to_string(),
        ))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (window, browser_type); // Suppress unused variable warnings
        Err(BrowserInfoError::PlatformError(
            "Unsupported platform".to_string(),
        ))
    }
}
