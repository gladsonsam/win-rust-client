//! # Input Injection Module
//!
//! Deserialises JSON control commands that arrive as **text frames** on the
//! WebSocket and executes them via the [`enigo`] input-simulation library
//! (v0.2+).
//!
//! ## Supported commands
//!
//! All commands are identified by a `"type"` discriminant field:
//!
//! ### `MouseMove`
//! Moves the cursor to an absolute screen position.
//! ```json
//! { "type": "MouseMove", "x": 960, "y": 540 }
//! ```
//!
//! ### `MouseClick`
//! Moves the cursor then performs a full press+release click.
//! The `"button"` field defaults to `"left"` when omitted.
//! ```json
//! { "type": "MouseClick", "x": 100, "y": 200, "button": "right" }
//! ```

use anyhow::{Context, Result};
use enigo::{Button, Coordinate, Direction, Enigo, Keyboard, Key, Mouse, Settings};
use serde::Deserialize;
use tracing::info;
use winrt_notification::Toast;

// ─────────────────────────────────────────────────────────────────────────────
// Wire types (deserialised from inbound JSON)
// ─────────────────────────────────────────────────────────────────────────────

/// Which mouse button to use for a click action.
#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

impl From<MouseButton> for Button {
    fn from(b: MouseButton) -> Self {
        match b {
            MouseButton::Left => Button::Left,
            MouseButton::Right => Button::Right,
            MouseButton::Middle => Button::Middle,
        }
    }
}

/// A control command received from the server over the WebSocket.
///
/// Serde's **internally tagged** representation is used so the JSON
/// `"type"` field selects the correct variant automatically.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ControlCommand {
    /// Move the OS cursor to an absolute screen coordinate.
    MouseMove { x: i32, y: i32 },

    /// Move to a coordinate and click a mouse button.
    MouseClick {
        x: i32,
        y: i32,
        /// Defaults to `"left"` when omitted in the JSON payload.
        #[serde(default)]
        button: MouseButton,
    },

    /// Type literal text into the currently focused window.
    ///
    /// Sent by the dashboard when remote control is enabled and the user types.
    TypeText { text: String },

    /// Press a special key (Enter, Backspace, etc).
    KeyPress { key: SpecialKey },

    /// Display a user-visible notification on the agent machine.
    Notify { title: String, message: String },
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SpecialKey {
    Enter,
    Backspace,
    Tab,
    Escape,
}

// ─────────────────────────────────────────────────────────────────────────────
// Controller
// ─────────────────────────────────────────────────────────────────────────────

/// Owns an [`Enigo`] context and dispatches inbound [`ControlCommand`]s.
///
/// Construct once at session start and reuse for the session lifetime.
/// Creating multiple [`Enigo`] instances simultaneously can cause issues
/// on some Windows driver backends.
pub struct InputController {
    enigo: Enigo,
}

impl InputController {
    /// Initialise the Enigo input backend.
    ///
    /// # Errors
    /// Fails if the platform input driver cannot be initialised
    /// (e.g. missing privileges on a locked desktop).
    pub fn new() -> Result<Self> {
        let enigo = Enigo::new(&Settings::default())
            .context("Failed to initialise Enigo input controller")?;
        Ok(Self { enigo })
    }

    /// Parse a JSON text payload and execute the encoded command.
    ///
    /// Unknown command types produce a deserialisation error which the caller
    /// should log and discard — *never* abort the session for bad input.
    ///
    /// # Errors
    /// - JSON deserialisation failure (malformed or unknown command).
    /// - Enigo platform error (input driver rejected the action).
    pub fn handle_command(&mut self, json: &str) -> Result<()> {
        let cmd: ControlCommand =
            serde_json::from_str(json).context("Invalid control command JSON")?;

        match cmd {
            // ── MouseMove ──────────────────────────────────────────────────
            ControlCommand::MouseMove { x, y } => {
                info!("→ MouseMove  x={x}  y={y}");
                self.enigo
                    .move_mouse(x, y, Coordinate::Abs)
                    .context("move_mouse failed")?;
            }

            // ── MouseClick ─────────────────────────────────────────────────
            ControlCommand::MouseClick { x, y, button } => {
                info!("→ MouseClick  x={x}  y={y}  button={button:?}");

                // Always move to the target before clicking so the click
                // lands on the intended element even if the cursor drifted.
                self.enigo
                    .move_mouse(x, y, Coordinate::Abs)
                    .context("move_mouse (pre-click) failed")?;

                // `Direction::Click` is a convenience wrapper for
                // Press + Release in a single call.
                self.enigo
                    .button(button.into(), Direction::Click)
                    .context("button click failed")?;
            }

            // ── Typing ───────────────────────────────────────────────────────
            ControlCommand::TypeText { text } => {
                if text.is_empty() {
                    return Ok(());
                }
                info!("→ TypeText  len={}", text.chars().count());
                self.enigo.text(&text).context("text failed")?;
            }

            ControlCommand::KeyPress { key } => {
                info!("→ KeyPress  key={key:?}");
                let k = match key {
                    SpecialKey::Enter => Key::Return,
                    SpecialKey::Backspace => Key::Backspace,
                    SpecialKey::Tab => Key::Tab,
                    SpecialKey::Escape => Key::Escape,
                };
                self.enigo.key(k, Direction::Click).context("key press failed")?;
            }

            // ── Notification ────────────────────────────────────────────────
            ControlCommand::Notify { title, message } => {
                let title = title.trim();
                let message = message.trim();
                if title.is_empty() && message.is_empty() {
                    return Ok(());
                }

                // Compatibility fallback: uses the PowerShell AUMID so toasts work
                // without any install/shortcut registration steps.
                let mut t = Toast::new(Toast::POWERSHELL_APP_ID);
                t = t.title(if title.is_empty() { "Sentinel" } else { title });
                if !message.is_empty() {
                    t = t.text1(message);
                }
                let _ = t.show();
            }
        }

        Ok(())
    }
}
