//! # browser-url
//!
//! Cross-platform library for retrieving active browser URL and detailed information.
//!
//! Built on top of `active-win-pos-rs` for reliable window detection, with specialized
//! browser information extraction capabilities.
//!
//! ## Quick Start
//!
//! ```rust
//! use browser_url::get_active_browser_url;
//!
//! match get_active_browser_url() {
//!     Ok(info) => {
//!         println!("URL: {}", info.url);
//!     }
//!     Err(e) => eprintln!("Error: {}", e),
//! }
//! ```

use active_win_pos_rs::get_active_window;
use serde::{Deserialize, Serialize};

pub mod browser_detection;
pub mod error;
pub mod url_extraction;

pub mod platform;

pub use error::BrowserInfoError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserInfo {
    pub url: String,
    pub title: String,
    pub browser_name: String,
    pub browser_type: BrowserType,
    // pub is_incognito: bool,
    pub process_id: u64,
    pub window_position: WindowPosition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BrowserType {
    Chrome,
    Firefox,
    Zen,
    Edge,
    Safari,
    Brave,
    Opera,
    Vivaldi,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WindowPosition {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

//================================================================================================
// procedure
//================================================================================================

/// Retrieve information about the currently active browser
///
/// This function combines window detection (via `active-win-pos-rs`) with
/// specialized browser information extraction.
///
/// # Examples
///
/// ```rust
/// use browser_url::get_active_browser_url;
///
/// match get_active_browser_url() {
///     Ok(info) => {
///         println!("URL: {}", info.url);
///         println!("Title: {}", info.title);
///         println!("Browser: {}", info.browser_name);
///         println!("Process ID: {}", info.process_id);
///         println!("Position: ({}, {})", info.window_position.x, info.window_position.y);
///         println!("Size: {}x{}", info.window_position.width, info.window_position.height);
///     }
///     Err(e) => eprintln!("Failed to get browser info: {}", e),
/// }
/// ```

pub fn get_active_browser_url() -> Result<BrowserInfo, BrowserInfoError> {
    if !is_browser_active() {
        return Err(BrowserInfoError::NotABrowser);
    }

    let window = get_active_window().map_err(|_| BrowserInfoError::WindowNotFound)?;

    let browser_type = browser_detection::classify_browser(&window)?;

    let url = url_extraction::extract_url(&window, &browser_type)?;

    Ok(BrowserInfo {
        url,
        title: window.title,
        browser_name: window.app_name,
        browser_type,
        // is_incognito: url.is_incognito,
        process_id: window.process_id,
        window_position: WindowPosition {
            x: window.position.x,
            y: window.position.y,
            width: window.position.width,
            height: window.position.height,
        },
    })
}


pub fn is_browser_active() -> bool {
    if let Ok(window) = get_active_window() {
        browser_detection::classify_browser(&window).is_ok()
    } else {
        false
    }
}