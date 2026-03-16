/// Thin wrapper around the `browser-url` crate.
///
/// Returns `None` silently for any error (browser not open, unsupported
/// browser, UIAutomation timeout, etc.) so callers never see noisy logs.
pub fn get_active_url() -> Option<browser_url::BrowserInfo> {
    match browser_url::get_active_browser_url() {
        Ok(info) if !info.url.trim().is_empty() => Some(info),
        _ => None,
    }
}
