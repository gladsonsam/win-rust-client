# browser-url

🚀 A Rust library for extracting the active browser's URL using native platform APIs.

Fast, lightweight, and easy-to-use — get the URL from the currently focused browser window in sub-millisecond time.

## ✨ Features

- ⚡ **Ultra Fast** — Uses native platform APIs (UI Automation on Windows) for sub-millisecond extraction
- 🌐 **Multi-Browser** — Chrome, Firefox, Brave, Zen (more coming soon)
- 🖥️ **Cross-Platform** — Windows (full support), macOS & Linux (planned)
- 📦 **Simple API** — Two functions: `is_browser_active()` and `get_active_browser_url()`

## 📦 Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
browser-url = "0.1.0"
```

Or via the CLI:

```bash
cargo add browser-url
```

## 🚀 Quick Start

```rust
use browser_url::{get_active_browser_url, is_browser_active};

fn main() {
    if !is_browser_active() {
        println!("No browser window is currently focused.");
        return;
    }

    match get_active_browser_url() {
        Ok(info) => {
            println!("URL:        {}", info.url);
            println!("Title:      {}", info.title);
            println!("Browser:    {} ({:?})", info.browser_name, info.browser_type);
            println!("Process ID: {}", info.process_id);
            println!("Position:   ({}, {})", info.window_position.x, info.window_position.y);
            println!("Size:       {}x{}", info.window_position.width, info.window_position.height);
        }
        Err(e) => eprintln!("Error: {e}"),
    }
}
```

## 🔧 API

### `is_browser_active() -> bool`

Returns `true` if the currently focused window is a recognized browser.

### `get_active_browser_url() -> Result<BrowserInfo, BrowserInfoError>`

Extracts full browser information from the active window, including the URL, title, browser type, process ID, and window position/size.

#### `BrowserInfo`

| Field             | Type             | Description                             |
| ----------------- | ---------------- | --------------------------------------- |
| `url`             | `String`         | The URL from the browser address bar    |
| `title`           | `String`         | The window title                        |
| `browser_name`    | `String`         | Application name (e.g. "Google Chrome") |
| `browser_type`    | `BrowserType`    | Enum variant (Chrome, Firefox, etc.)    |
| `process_id`      | `u64`            | OS process ID                           |
| `window_position` | `WindowPosition` | x, y, width, height                     |

### Supported Browsers

| Browser | Windows | macOS | Linux |
| ------- | ------- | ----- | ----- |
| Chrome  | ✅      | ⏳    | ⏳    |
| Firefox | ✅      | ⏳    | ⏳    |
| Zen     | ✅      | ⏳    | ⏳    |
| Brave   | 🔜      | ⏳    | ⏳    |
| Edge    | 🔜      | ⏳    | ⏳    |
| Safari  | —       | ⏳    | —     |
| Opera   | 🔜      | ⏳    | ⏳    |
| Vivaldi | 🔜      | ⏳    | ⏳    |

✅ Supported &nbsp; 🔜 Detected, extraction coming &nbsp; ⏳ Planned &nbsp; — N/A

## 🔍 Examples

Run the included example (switch to a browser window when prompted):

```bash
cargo run --example basic_usage
```

## 🤝 Contributing

Contributions are welcome! Here's how to get started:

1. Clone the repository
2. Install Rust (1.90+)
3. Run: `cargo build`
4. Submit a pull request

### Roadmap

- [ ] Test and Verify: Brave / Edge / Vivaldi URL extraction on Windows
- [ ] macOS support
- [ ] Linux support
- [ ] Incognito / private mode detection

## 🙏 Acknowledgements

This project builds on the shoulders of these excellent crates:

- **[browser-info](https://github.com/frkavka/browser-info)** — Inspiration for the project's scope and API design around browser information retrieval.
- **[active-win-pos-rs](https://crates.io/crates/active-win-pos-rs)** — Cross-platform active window detection. Used to identify the focused window before extracting browser info.
- **[uiautomation](https://crates.io/crates/uiautomation)** — Rust bindings for Windows UI Automation. Powers the URL extraction from browser address bars on Windows.

## 📄 License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.

---

<div align="center">
  <sub>Built with ❤️ by <a href="https://github.com/KrishnaV2">Krishna</a></sub>
</div>
