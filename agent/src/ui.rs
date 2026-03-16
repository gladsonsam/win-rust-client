//! eframe application: system-tray icon + password-gated settings window.
//!
//! ## Why we avoid ViewportCommand::Visible(false)
//!
//! Calling Visible(false) causes eframe/winit to suspend the render loop
//! entirely — update() is never called again, so tray events pile up in
//! our channel unprocessed.  Instead we keep the window alive at all times
//! and simply move it off-screen when "hidden":
//!
//!   Hidden  → OuterPosition(-32000, -32000)   window off all monitors
//!   Visible → OuterPosition(center of screen) window on screen
//!
//! WS_EX_TOOLWINDOW (set via the Win32 API once on startup) hides the
//! window from the taskbar and Alt-Tab switcher so the user never sees it
//! accidentally.

use std::sync::{mpsc, Arc, Mutex};

use eframe::egui;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use crate::config::{self, AgentStatus, Config};

// ─── Screen-position constants ────────────────────────────────────────────────

/// Window position when "hidden": far off every physical monitor.
const POS_HIDDEN: egui::Pos2 = egui::pos2(-32000.0, -32000.0);
/// Window position when first shown (reasonable default; user can move it).
const POS_VISIBLE: egui::Pos2 = egui::pos2(200.0, 120.0);

// ─── Internal tray event type ─────────────────────────────────────────────────

enum TrayMsg {
    Show,        // left-click or "Settings" menu item
    Quit,        // "Exit" menu item
}

// ─── View state ───────────────────────────────────────────────────────────────

#[derive(Debug, Default, PartialEq)]
enum View {
    #[default]
    Hidden,
    PasswordPrompt,
    Settings,
}

// ─── Application ─────────────────────────────────────────────────────────────

pub struct AgentApp {
    view:     View,
    _tray:    TrayIcon,
    tray_rx:  mpsc::Receiver<TrayMsg>,

    icon_green:  tray_icon::Icon,
    icon_yellow: tray_icon::Icon,
    icon_red:    tray_icon::Icon,
    last_status: AgentStatus,

    saved_config:      Config,
    f_server_url:      String,
    f_agent_name:      String,
    f_agent_password:  String,
    f_new_ui_pass:     String,
    f_confirm_ui_pass: String,

    p_input:           String,
    p_error:           bool,
    p_focus_requested: bool,

    form_msg:    Option<(String, bool)>,
    is_quitting:       bool,
    /// Set WS_EX_TOOLWINDOW exactly once on the first update() frame.
    toolwindow_applied: bool,

    config_tx:    tokio::sync::watch::Sender<Option<Config>>,
    agent_status: Arc<Mutex<AgentStatus>>,
}

impl AgentApp {
    pub fn new(
        cc:             &eframe::CreationContext<'_>,
        initial_config: Config,
        config_tx:      tokio::sync::watch::Sender<Option<Config>>,
        agent_status:   Arc<Mutex<AgentStatus>>,
    ) -> Self {
        // ── Context menu ──────────────────────────────────────────────────
        let settings_item = MenuItem::new("Settings", true, None);
        let quit_item     = MenuItem::new("Exit",     true, None);
        let s_id          = settings_item.id().clone();
        let q_id          = quit_item.id().clone();

        let menu = Menu::new();
        let _ = menu.append_items(&[
            &settings_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ]);

        // ── Tray icons ────────────────────────────────────────────────────
        let icon_green  = make_circle_icon(46,  204, 113);
        let icon_yellow = make_circle_icon(241, 196,  15);
        let icon_red    = make_circle_icon(231,  76,  60);

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Agent: Disconnected")
            .with_icon(icon_red.clone())
            .build()
            .expect("Failed to build tray icon");

        // ── Event channel ─────────────────────────────────────────────────
        //
        // set_event_handler is the SOLE consumer of events in tray-icon —
        // receiver().try_recv() will always be empty once a handler is set.
        // We forward events into our own channel and call request_repaint()
        // so update() wakes up even if the window is off-screen.
        let (tx, tray_rx) = mpsc::channel::<TrayMsg>();

        {
            let tx  = tx.clone();
            let ctx = cc.egui_ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button:       MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = e
                {
                    let _ = tx.send(TrayMsg::Show);
                    ctx.request_repaint();
                }
            }));
        }

        {
            let ctx = cc.egui_ctx.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let msg = if e.id == s_id {
                    Some(TrayMsg::Show)
                } else if e.id == q_id {
                    Some(TrayMsg::Quit)
                } else {
                    None
                };
                if let Some(m) = msg {
                    let _ = tx.send(m);
                    ctx.request_repaint();
                }
            }));
        }

        Self {
            view: View::Hidden,
            _tray: tray,
            tray_rx,
            icon_green,
            icon_yellow,
            icon_red,
            last_status: AgentStatus::Disconnected,
            f_server_url:      initial_config.server_url.clone(),
            f_agent_name:      initial_config.agent_name.clone(),
            f_agent_password:  initial_config.agent_password.clone(),
            saved_config:      initial_config,
            f_new_ui_pass:     String::new(),
            f_confirm_ui_pass: String::new(),
            p_input:           String::new(),
            p_error:           false,
            p_focus_requested: false,
            form_msg:           None,
            is_quitting:        false,
            toolwindow_applied: false,
            config_tx,
            agent_status,
        }
    }

    fn handle_tray_events(&mut self) {
        while let Ok(msg) = self.tray_rx.try_recv() {
            match msg {
                TrayMsg::Show => self.open_window(),
                TrayMsg::Quit => self.is_quitting = true,
            }
        }
    }

    fn sync_tray_status(&mut self) {
        let status = self.agent_status.lock().unwrap().clone();
        if status == self.last_status { return; }
        self.last_status = status.clone();

        let icon: tray_icon::Icon = match &status {
            AgentStatus::Connected    => self.icon_green.clone(),
            AgentStatus::Connecting   => self.icon_yellow.clone(),
            _                         => self.icon_red.clone(),
        };
        let tip: String = match &status {
            AgentStatus::Connected    => "Agent: Connected".into(),
            AgentStatus::Connecting   => "Agent: Connecting…".into(),
            AgentStatus::Disconnected => "Agent: Disconnected".into(),
            AgentStatus::Error(e)     => format!("Agent error: {}", &e[..e.len().min(60)]),
        };
        let _ = self._tray.set_icon(Some(icon));
        let _ = self._tray.set_tooltip(Some(tip));
    }

    fn open_window(&mut self) {
        if self.view == View::Hidden {
            self.p_input.clear();
            self.p_error           = false;
            self.p_focus_requested = false;
            self.view              = View::PasswordPrompt;
        }
    }

    fn close_window(&mut self) {
        self.view     = View::Hidden;
        self.form_msg = None;
    }

    fn save_settings(&mut self) {
        if !self.f_new_ui_pass.is_empty()
            && self.f_new_ui_pass != self.f_confirm_ui_pass
        {
            self.form_msg = Some(("Passwords don't match".into(), true));
            return;
        }
        let ui_hash = if self.f_new_ui_pass.is_empty() {
            self.saved_config.ui_password_hash.clone()
        } else {
            config::hash_password(&self.f_new_ui_pass)
        };
        let cfg = Config {
            server_url:       self.f_server_url.trim().to_string(),
            agent_name:       self.f_agent_name.trim().to_string(),
            agent_password:   self.f_agent_password.clone(),
            ui_password_hash: ui_hash,
        };
        match config::save_config(&cfg) {
            Ok(()) => {
                self.saved_config = cfg.clone();
                let val = if cfg.server_url.is_empty() { None } else { Some(cfg) };
                let _ = self.config_tx.send(val);
                self.f_new_ui_pass.clear();
                self.f_confirm_ui_pass.clear();
                self.form_msg = Some(("✓ Settings saved.".into(), false));
            }
            Err(e) => {
                self.form_msg = Some((format!("✗ Save failed: {e}"), true));
            }
        }
    }
}

// ─── eframe::App ─────────────────────────────────────────────────────────────

impl eframe::App for AgentApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply WS_EX_TOOLWINDOW once on the first frame (window now exists).
        if !self.toolwindow_applied {
            self.toolwindow_applied = true;
            #[cfg(target_os = "windows")]
            apply_toolwindow_by_title();
        }

        let was_hidden = self.view == View::Hidden;

        // Close button → hide (not quit) unless we're doing a real exit
        if ctx.input(|i| i.viewport().close_requested()) && !self.is_quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_window();
        }

        // Drain tray/menu events (may flip self.view)
        self.handle_tray_events();
        self.sync_tray_status();

        // ── Render ────────────────────────────────────────────────────────
        match self.view {
            View::Hidden => {
                egui::CentralPanel::default().show(ctx, |_ui| {});
            }

            View::PasswordPrompt => {
                let saved_hash = self.saved_config.ui_password_hash.clone();
                let mut intent: Option<bool> = None;

                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.heading("🔒  Agent Settings");
                        ui.add_space(10.0);
                        ui.label("Enter the UI access password:");
                        ui.add_space(14.0);

                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.p_input)
                                .password(true)
                                .hint_text("password")
                                .desired_width(220.0),
                        );
                        if !self.p_focus_requested {
                            resp.request_focus();
                            self.p_focus_requested = true;
                        }

                        if self.p_error {
                            ui.add_space(6.0);
                            ui.colored_label(
                                egui::Color32::from_rgb(231, 76, 60),
                                "⚠  Wrong password – try again.",
                            );
                        }

                        ui.add_space(14.0);
                        let enter   = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let clicked = ui
                            .add_sized([120.0, 32.0], egui::Button::new("🔓  Unlock"))
                            .clicked();

                        if (enter || clicked) && intent.is_none() {
                            intent = Some(config::hash_password(&self.p_input) == saved_hash);
                        }
                    });
                });

                match intent {
                    Some(true) => {
                        self.p_input.clear();
                        self.p_error = false;
                        self.f_server_url     = self.saved_config.server_url.clone();
                        self.f_agent_name     = self.saved_config.agent_name.clone();
                        self.f_agent_password = self.saved_config.agent_password.clone();
                        self.f_new_ui_pass.clear();
                        self.f_confirm_ui_pass.clear();
                        self.form_msg = None;
                        self.view = View::Settings;
                    }
                    Some(false) => {
                        self.p_error = true;
                        self.p_input.clear();
                    }
                    None => {}
                }
            }

            View::Settings => {
                let status = self.agent_status.lock().unwrap().clone();
                let mut action: Option<SettingsAction> = None;

                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add_space(4.0);
                        ui.heading("⚙  Agent Configuration");
                        ui.separator();
                        ui.add_space(8.0);

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.label(egui::RichText::new("Connection").strong());
                            ui.add_space(4.0);
                            egui::Grid::new("conn").num_columns(2).spacing([8.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label("Server URL:");
                                    ui.add(egui::TextEdit::singleline(&mut self.f_server_url)
                                        .hint_text("ws://192.168.1.100:9000/ws/agent")
                                        .desired_width(290.0));
                                    ui.end_row();

                                    ui.label("Agent Name:");
                                    ui.add(egui::TextEdit::singleline(&mut self.f_agent_name)
                                        .hint_text("DESKTOP-ABC123")
                                        .desired_width(290.0));
                                    ui.end_row();

                                    ui.label("Agent Password:");
                                    ui.add(egui::TextEdit::singleline(&mut self.f_agent_password)
                                        .password(true).hint_text("(optional)")
                                        .desired_width(290.0));
                                    ui.end_row();
                                });
                        });

                        ui.add_space(8.0);

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.label(egui::RichText::new("UI Access Password").strong());
                            ui.label(egui::RichText::new("Leave blank to keep current.")
                                .small().color(egui::Color32::GRAY));
                            ui.add_space(4.0);
                            egui::Grid::new("sec").num_columns(2).spacing([8.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label("New Password:");
                                    ui.add(egui::TextEdit::singleline(&mut self.f_new_ui_pass)
                                        .password(true)
                                        .hint_text("leave blank = no change")
                                        .desired_width(250.0));
                                    ui.end_row();

                                    ui.label("Confirm:");
                                    ui.add(egui::TextEdit::singleline(&mut self.f_confirm_ui_pass)
                                        .password(true).desired_width(250.0));
                                    ui.end_row();
                                });
                        });

                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            ui.label("Status:");
                            let (color, label) = match &status {
                                AgentStatus::Connected    => (egui::Color32::from_rgb(46,204,113), "● Connected"),
                                AgentStatus::Connecting   => (egui::Color32::from_rgb(241,196,15), "◌ Connecting…"),
                                AgentStatus::Disconnected => (egui::Color32::from_rgb(149,165,166), "○ Disconnected"),
                                AgentStatus::Error(_)     => (egui::Color32::from_rgb(231,76,60),  "✗ Error"),
                            };
                            ui.colored_label(color, label);
                        });

                        if let Some((msg, is_err)) = &self.form_msg {
                            ui.add_space(4.0);
                            let c = if *is_err { egui::Color32::from_rgb(231,76,60) }
                                    else        { egui::Color32::from_rgb(46,204,113) };
                            ui.colored_label(c, msg);
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            if ui.add_sized([100.0, 30.0], egui::Button::new("💾  Save")).clicked() {
                                action = Some(SettingsAction::Save);
                            }
                            ui.add_space(8.0);
                            if ui.add_sized([100.0, 30.0], egui::Button::new("✕  Close")).clicked() {
                                action = Some(SettingsAction::Close);
                            }
                        });
                    });
                });

                match action {
                    Some(SettingsAction::Save)  => self.save_settings(),
                    Some(SettingsAction::Close) => self.close_window(),
                    None => {}
                }
            }
        }

        // ── Move window on/off screen AFTER processing events ─────────────
        //
        // We never call Visible(false) — that suspends update().
        // Instead we keep the window running but positioned off all monitors.
        let now_hidden = self.view == View::Hidden;

        if now_hidden {
            // Keep it parked off-screen so it never flashes into view
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(POS_HIDDEN));
        } else if was_hidden && !now_hidden {
            // Transition: hidden → visible — move onto screen and focus
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(POS_VISIBLE));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        if self.is_quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Keep ticking even off-screen so status updates propagate to tray
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum SettingsAction { Save, Close }

/// Hide our window from the taskbar and Alt-Tab by applying WS_EX_TOOLWINDOW.
///
/// We locate our HWND via FindWindowW (by title) because eframe 0.33 does not
/// expose raw_window_handle publicly.  Called once from the first update() so
/// the Win32 window definitely exists by then.
#[cfg(target_os = "windows")]
fn apply_toolwindow_by_title() {
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongW, SetWindowLongW,
        GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    // Null-terminated UTF-16 title
    let title: Vec<u16> = "Agent Settings\0".encode_utf16().collect();
    unsafe {
        // In windows 0.58, FindWindowW returns Result<HWND>
        if let Ok(hwnd) = FindWindowW(
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR(title.as_ptr()),
        ) {
            let ex  = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let new = (ex | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0;
            SetWindowLongW(hwnd, GWL_EXSTYLE, new as i32);
        }
    }
}

/// Generate a 32×32 RGBA filled circle for use as a tray icon.
fn make_circle_icon(r: u8, g: u8, b: u8) -> tray_icon::Icon {
    const S: u32 = 32;
    let mut px = vec![0u8; (S * S * 4) as usize];
    let c = S as f32 / 2.0 - 0.5;
    for y in 0..S {
        for x in 0..S {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let i = ((y * S + x) * 4) as usize;
            if d <= 14.5 {
                let f = if d >= 12.5 { 0.65_f32 } else { 1.0 };
                px[i]     = (r as f32 * f) as u8;
                px[i + 1] = (g as f32 * f) as u8;
                px[i + 2] = (b as f32 * f) as u8;
                px[i + 3] = 255;
            }
        }
    }
    tray_icon::Icon::from_rgba(px, S, S).expect("valid tray icon")
}
