use crate::{BrowserInfoError, BrowserType};
use active_win_pos_rs::ActiveWindow;

pub fn classify_browser(window: &ActiveWindow) -> Result<BrowserType, BrowserInfoError> {
    let app_name = window.app_name.to_lowercase();

    let process_path = window.process_path.to_str().unwrap_or("").to_lowercase();

    if app_name.contains("chrome") && !app_name.contains("edge") {
        Ok(BrowserType::Chrome)
    } else if app_name.contains("firefox") {
        Ok(BrowserType::Firefox)
    } else if app_name.contains("zen") {
        Ok(BrowserType::Zen)
    } else if app_name.contains("msedge") || app_name.contains("edge") {
        Ok(BrowserType::Edge)
    } else if app_name.contains("safari") {
        Ok(BrowserType::Safari)
    } else if app_name.contains("brave") {
        Ok(BrowserType::Brave)
    } else if app_name.contains("opera") {
        Ok(BrowserType::Opera)
    } else if app_name.contains("vivaldi") {
        Ok(BrowserType::Vivaldi)
    } else if is_browser_by_path(&process_path) {
        detect_browser_from_path(&process_path)
    } else {
        Err(BrowserInfoError::NotABrowser)
    }
}

fn is_browser_by_path(path: &str) -> bool {
    let browser_indicators = [
        "chrome", "firefox", "edge", "safari", "brave", "opera", "vivaldi",
    ];
    browser_indicators
        .iter()
        .any(|&indicator| path.contains(indicator))
}

fn detect_browser_from_path(path: &str) -> Result<BrowserType, BrowserInfoError> {
    if path.contains("chrome") {
        Ok(BrowserType::Chrome)
    } else if path.contains("firefox") {
        Ok(BrowserType::Firefox)
    } else if path.contains("edge") {
        Ok(BrowserType::Edge)
    } else {
        Ok(BrowserType::Unknown("detected_from_path".to_string()))
    }
}
