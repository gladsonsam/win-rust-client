use crate::{BrowserInfoError, BrowserType};
use active_win_pos_rs::ActiveWindow;
use uiautomation::UIAutomation;
use uiautomation::types::UIProperty;
use uiautomation::types::ControlType;

pub fn extract_url(
    window: &ActiveWindow,
    browser_type: &BrowserType,
) -> Result<String, BrowserInfoError> {
    let automation = UIAutomation::new().unwrap();
    let root = automation.get_root_element().unwrap();

    match browser_type {
        BrowserType::Firefox | BrowserType::Zen => {
            let matcher = automation
                .create_matcher()
                .from(root)
                .timeout(10000)
                .name(&window.title);

            if let Ok(browser) = matcher.find_first() {
                let matcher = automation
                    .create_matcher()
                    .from(browser)
                    .timeout(10000)
                    .control_type(ControlType::Edit);

                if let Ok(ele) = matcher.find_first() {
                    let url_variant = ele.get_property_value(UIProperty::ValueValue)
                        .unwrap_or_default();
                    let url = url_variant.get_string().unwrap_or_default();
                    if url == "" {
                        return Err(BrowserInfoError::UrlExtractionFailed(window.app_name.to_string()))
                    }
                    return Ok(url)
                }
            }
            return Err(BrowserInfoError::UrlExtractionFailed(window.app_name.to_string()))
        }
        BrowserType::Chrome => {
            let matcher = automation
                .create_matcher()
                .from(root)
                .timeout(10000)
                .name(&window.title);

            if let Ok(browser) = matcher.find_first() {
                let matcher = automation
                    .create_matcher()
                    .from(browser)
                    .timeout(10000)
                    .classname("ToolbarView");

                if let Ok(toolbar) = matcher.find_first() {
                    let matcher = automation
                        .create_matcher()
                        .from(toolbar)
                        .timeout(10000)
                        .classname("LocationBarView");

                    if let Ok(address_bar) = matcher.find_first() {

                        let matcher = automation
                            .create_matcher()
                            .from(address_bar)
                            .timeout(10000)
                            .control_type(uiautomation::types::ControlType::Edit);

                        if let Ok(ele) = matcher.find_first() {
                            let url_variant = ele.get_property_value(UIProperty::ValueValue)
                                .unwrap_or_default();
                            let url = url_variant.get_string().unwrap_or_default();
                            if url == "" {
                                return Err(BrowserInfoError::UrlExtractionFailed(window.app_name.to_string()))
                            }
                            return Ok(url)
                        }
                    }
                }
            }
            return Err(BrowserInfoError::UrlExtractionFailed(window.app_name.to_string()))
        }
        _ => {
            return Err(BrowserInfoError::BrowserDetectionFailed(window.app_name.to_string()))
        }
    }
}
