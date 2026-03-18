//! Cross-platform settings popup built with egui / eframe.
//!
//! ## How to access settings
//!
//! The process has no taskbar entry.  Press **Ctrl+Shift+F12** to open the
//! password-protected settings window.  On first run (no password configured
//! yet) the password step is skipped and the settings window opens directly.
//!
//! ## Architecture
//!
//! `eframe` owns the main thread and event loop.  A `global-hotkey` manager
//! registers the global hotkey; its events are polled inside `update()`.
//!
//! The window starts **hidden** (except on first run) and is shown/hidden via
//! `ViewportCommand::Visible`.  The close button hides rather than quits; only
//! the "Exit Agent" button terminates the process.
//!
//! The agent WebSocket loop runs in a separate thread (spawned by `main`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, Color32, RichText, ViewportCommand};
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use tracing::error;

use crate::config::{self, AgentStatus, Config};

// ─── Screen state ─────────────────────────────────────────────────────────────

enum UiScreen {
    Hidden,
    PasswordPrompt { input: String, error: bool },
    Settings,
}

// ─── Inner egui app ───────────────────────────────────────────────────────────

struct AgentUiApp {
    screen: UiScreen,
    should_exit: bool,

    // Settings form fields
    url: String,
    name: String,
    agent_pw: String,
    new_pw: String,
    confirm_pw: String,
    form_msg: Option<(String, bool)>, // (text, is_error)

    // Shared cross-thread state
    config: Config,
    config_tx: tokio::sync::watch::Sender<Option<Config>>,
    agent_status: Arc<Mutex<AgentStatus>>,

    // Hotkey manager must stay alive for the hotkey to remain registered
    _hotkey_mgr: GlobalHotKeyManager,
    hotkey_id: u32,
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub struct AgentApp {
    initial_config: Config,
    config_tx: tokio::sync::watch::Sender<Option<Config>>,
    agent_status: Arc<Mutex<AgentStatus>>,
    show_on_startup: bool,
}

impl AgentApp {
    pub fn new(
        initial_config: Config,
        config_tx: tokio::sync::watch::Sender<Option<Config>>,
        agent_status: Arc<Mutex<AgentStatus>>,
    ) -> Self {
        Self {
            initial_config,
            config_tx,
            agent_status,
            show_on_startup: false,
        }
    }

    /// Force the settings window to be visible on startup (useful for
    /// troubleshooting / first-time installs on Windows).
    pub fn with_show_on_startup(mut self, show: bool) -> Self {
        self.show_on_startup = show;
        self
    }

    pub fn run(self) -> i32 {
        let mgr = GlobalHotKeyManager::new().expect("Failed to create global hotkey manager");
        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::F12);
        mgr.register(hotkey)
            .expect("Failed to register hotkey Ctrl+Shift+F12");
        let hotkey_id = hotkey.id();

        let first_run = self.initial_config.server_url.is_empty();
        let show_on_startup = self.show_on_startup;
        let cfg = self.initial_config.clone();
        let url = cfg.server_url.clone();
        let name = cfg.agent_name.clone();
        let agent_pw = cfg.agent_password.clone();
        let config_tx = self.config_tx;
        let agent_status = self.agent_status;

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Agent Settings")
                .with_inner_size([500.0, 460.0])
                .with_min_inner_size([500.0, 400.0])
                .with_resizable(false)
                .with_visible(first_run || show_on_startup)
                // If we explicitly show the window (or it's first run), keep it
                // discoverable via the taskbar. Otherwise the agent runs "headless"
                // and is reopened via hotkey only.
                .with_taskbar(first_run || show_on_startup),
            // Use wgpu instead of OpenGL so the app works on VMs and machines
            // with no real GPU.  On Windows the backend priority is:
            //   1. Direct3D 12 with a real GPU
            //   2. Direct3D 12 via WARP (Windows built-in software rasterizer,
            //      zero GPU required — always available on Windows 10+)
            // GL is deliberately excluded; it caused the "OpenGL 2.0+" crash on VMs.
            wgpu_options: WgpuConfiguration {
                wgpu_setup: WgpuSetup::CreateNew {
                    // Vulkan can be missing/broken on Windows VMs and can prevent
                    // successful init. DX12 is enough here (and will fall back to WARP).
                    // Note: `wgpu` 23.x does not expose a DX11 backend flag.
                    // If the process is running in a non-interactive Windows
                    // session (e.g. service / scheduled task "run whether user
                    // is logged on or not"), surface creation will fail for any
                    // backend. In that case run with `--no-ui` and configure via
                    // `AGENT_SERVER_URL` / `AGENT_NAME` / `AGENT_PASSWORD`.
                    supported_backends: eframe::wgpu::Backends::DX12,
                    power_preference: eframe::wgpu::PowerPreference::None,
                    device_descriptor: std::sync::Arc::new(|_adapter| {
                        eframe::wgpu::DeviceDescriptor::default()
                    }),
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let r = eframe::run_native(
            "Agent Settings",
            options,
            Box::new(move |cc| {
                setup_fonts_and_style(&cc.egui_ctx);
                Ok(Box::new(AgentUiApp {
                    screen: if first_run {
                        UiScreen::Settings
                    } else {
                        UiScreen::Hidden
                    },
                    should_exit: false,
                    url,
                    name,
                    agent_pw,
                    new_pw: String::new(),
                    confirm_pw: String::new(),
                    form_msg: None,
                    config: cfg,
                    config_tx,
                    agent_status,
                    _hotkey_mgr: mgr,
                    hotkey_id,
                }))
            }),
        );

        if let Err(e) = r {
            // Common in VMs / headless sessions (SSH/WinRM/service) where there's no
            // interactive desktop to create a window surface on.
            error!(
                "UI failed to start (running headless): {e}. You can still configure the agent by editing {:?}",
                config::config_path()
            );
            // Keep the main thread alive so the agent thread keeps running.
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        0
    }
}

// ─── eframe::App impl ────────────────────────────────────────────────────────

impl eframe::App for AgentUiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll global hotkey events
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id == self.hotkey_id {
                self.on_hotkey(ctx);
            }
        }

        // Intercept the native close button: hide instead of quit
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.should_exit {
                std::process::exit(0);
            }
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.screen = UiScreen::Hidden;
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }

        match self.screen {
            UiScreen::Hidden => {
                // Keep the event loop alive so hotkey events are processed.
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            UiScreen::PasswordPrompt { .. } => self.show_password_dialog(ctx),
            UiScreen::Settings => self.show_settings(ctx),
        }
    }
}

// ─── Hotkey handler ───────────────────────────────────────────────────────────

impl AgentUiApp {
    fn on_hotkey(&mut self, ctx: &egui::Context) {
        let empty_pw_hash = config::hash_password("");
        let has_password = !self.config.ui_password_hash.is_empty()
            && self.config.ui_password_hash != empty_pw_hash;

        self.screen = if has_password {
            UiScreen::PasswordPrompt {
                input: String::new(),
                error: false,
            }
        } else {
            UiScreen::Settings
        };

        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }
}

// ─── Password dialog ──────────────────────────────────────────────────────────

impl AgentUiApp {
    fn show_password_dialog(&mut self, ctx: &egui::Context) {
        // Extract what we need without holding a borrow on self.screen.
        let (mut input, error_flag) = match &self.screen {
            UiScreen::PasswordPrompt { input, error } => (input.clone(), *error),
            _ => return,
        };

        let mut do_unlock = false;
        let mut do_cancel = false;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(50.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("🔒  Agent Settings").size(22.0).strong());
                ui.add_space(20.0);

                ui.label("Enter UI access password:");
                ui.add_space(8.0);

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut input)
                        .password(true)
                        .desired_width(240.0)
                        .hint_text("Password"),
                );

                // Submit on Enter key while the field is focused
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    do_unlock = true;
                }

                if error_flag {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("⚠  Wrong password – try again.")
                            .color(Color32::from_rgb(220, 80, 80)),
                    );
                }

                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    // Manually centre the two buttons
                    ui.add_space(50.0);
                    if ui
                        .add_sized([100.0, 32.0], egui::Button::new("  Unlock  "))
                        .clicked()
                    {
                        do_unlock = true;
                    }
                    ui.add_space(12.0);
                    if ui
                        .add_sized([100.0, 32.0], egui::Button::new("  Cancel  "))
                        .clicked()
                    {
                        do_cancel = true;
                    }
                });
            });
        });

        // Write the (possibly edited) input back into the screen state
        if let UiScreen::PasswordPrompt {
            input: ref mut stored,
            ..
        } = self.screen
        {
            *stored = input.clone();
        }

        if do_unlock {
            let hash = self.config.ui_password_hash.clone();
            if config::hash_password(&input) == hash {
                self.screen = UiScreen::Settings;
            } else {
                self.screen = UiScreen::PasswordPrompt {
                    input: String::new(),
                    error: true,
                };
            }
        } else if do_cancel {
            self.screen = UiScreen::Hidden;
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
    }
}

// ─── Settings window ──────────────────────────────────────────────────────────

impl AgentUiApp {
    fn show_settings(&mut self, ctx: &egui::Context) {
        // Refresh status every 2 s without busy-polling
        ctx.request_repaint_after(Duration::from_secs(2));

        let (status_text, status_color) = {
            let s = self.agent_status.lock().unwrap().clone();
            match s {
                AgentStatus::Connected => {
                    ("●  Connected".to_string(), Color32::from_rgb(80, 210, 110))
                }
                AgentStatus::Connecting => (
                    "●  Connecting…".to_string(),
                    Color32::from_rgb(230, 190, 30),
                ),
                AgentStatus::Disconnected => (
                    "●  Disconnected".to_string(),
                    Color32::from_rgb(160, 160, 160),
                ),
                AgentStatus::Error(ref e) => {
                    (format!("●  Error: {e}"), Color32::from_rgb(220, 80, 80))
                }
            }
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Agent Settings").size(20.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(status_text).color(status_color).size(13.0));
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(8.0);

            // ── Connection ──────────────────────────────────────────────
            ui.label(RichText::new("Connection").strong());
            ui.add_space(6.0);

            egui::Grid::new("conn_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .min_col_width(110.0)
                .show(ui, |ui| {
                    ui.label("Server URL");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.url)
                            .desired_width(310.0)
                            .hint_text("ws://host:port/ws/agent"),
                    );
                    ui.end_row();

                    ui.label("Agent Name");
                    ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(310.0));
                    ui.end_row();

                    ui.label("Agent Password");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.agent_pw)
                            .password(true)
                            .desired_width(310.0),
                    );
                    ui.end_row();
                });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // ── UI Password (collapsible) ────────────────────────────────
            egui::CollapsingHeader::new(RichText::new("UI Access Password").strong())
                .default_open(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Leave blank to keep the current password.")
                            .small()
                            .color(Color32::GRAY),
                    );
                    ui.add_space(6.0);

                    egui::Grid::new("pw_grid")
                        .num_columns(2)
                        .spacing([10.0, 8.0])
                        .min_col_width(110.0)
                        .show(ui, |ui| {
                            ui.label("New Password");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_pw)
                                    .password(true)
                                    .desired_width(280.0),
                            );
                            ui.end_row();

                            ui.label("Confirm");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.confirm_pw)
                                    .password(true)
                                    .desired_width(280.0),
                            );
                            ui.end_row();
                        });
                    ui.add_space(4.0);
                });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // ── Buttons row ─────────────────────────────────────────────
            ui.horizontal(|ui| {
                if ui
                    .add_sized([100.0, 32.0], egui::Button::new("💾  Save"))
                    .clicked()
                {
                    self.do_save();
                }
                ui.add_space(6.0);
                if ui
                    .add_sized([90.0, 32.0], egui::Button::new("✖  Close"))
                    .clicked()
                {
                    self.screen = UiScreen::Hidden;
                    ctx.send_viewport_cmd(ViewportCommand::Visible(false));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let exit_btn = egui::Button::new(
                        RichText::new("⏻  Exit Agent").color(Color32::from_rgb(220, 80, 80)),
                    );
                    if ui.add_sized([110.0, 32.0], exit_btn).clicked() {
                        std::process::exit(0);
                    }
                });
            });

            // ── Save result message ─────────────────────────────────────
            if let Some((msg, is_error)) = &self.form_msg {
                ui.add_space(8.0);
                let color = if *is_error {
                    Color32::from_rgb(220, 80, 80)
                } else {
                    Color32::from_rgb(80, 210, 110)
                };
                ui.label(RichText::new(msg).color(color));
            }

            // ── Hint at the bottom ──────────────────────────────────────
            ui.add_space(6.0);
            ui.label(
                RichText::new("Hotkey to reopen: Ctrl+Shift+F12")
                    .small()
                    .color(Color32::GRAY),
            );
        });
    }

    fn do_save(&mut self) {
        if !self.new_pw.is_empty() && self.new_pw != self.confirm_pw {
            self.form_msg = Some(("⚠  Passwords don't match".into(), true));
            return;
        }

        let ui_hash = if self.new_pw.is_empty() {
            self.config.ui_password_hash.clone()
        } else {
            config::hash_password(&self.new_pw)
        };

        let new_cfg = Config {
            server_url: self.url.trim().to_string(),
            agent_name: self.name.trim().to_string(),
            agent_password: self.agent_pw.clone(),
            ui_password_hash: ui_hash,
        };

        match config::save_config(&new_cfg) {
            Ok(()) => {
                let _ = self.config_tx.send(Some(new_cfg.clone()));
                self.config = new_cfg;
                self.new_pw = String::new();
                self.confirm_pw = String::new();
                self.form_msg = Some(("✓  Settings saved".into(), false));
            }
            Err(e) => {
                self.form_msg = Some((format!("⚠  Save failed: {e}"), true));
            }
        }
    }
}

// ─── Fonts and style setup ────────────────────────────────────────────────────

fn setup_fonts_and_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Comfortable spacing and slightly rounder widgets
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(16.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.visuals.window_rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.active.rounding = egui::Rounding::same(4.0);

    ctx.set_style(style);
    ctx.set_pixels_per_point(1.0);
}
