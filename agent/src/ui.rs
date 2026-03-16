//! eframe application: system-tray icon + password-gated settings window.
//!
//! ## Architecture
//!
//! The window is always alive (eframe needs a window to run its render loop),
//! but it is hidden via Win32 `ShowWindow(SW_HIDE)` so it is completely
//! invisible: no taskbar button, not in Alt-Tab, not on screen.
//! `update()` keeps ticking thanks to `request_repaint_after()`.
//!
//! `WS_EX_TOOLWINDOW` (applied once on the first successful `FindWindowW`) is
//! a belt-and-suspenders measure to keep the window out of the taskbar even
//! while it is physically visible.
//!
//! ## Interaction model
//!
//!   Double-click tray icon  →  password prompt  →  settings panel
//!   "Settings" context menu →  same
//!   "Exit" context menu     →  password prompt  →  quit (after correct pw)

use std::sync::{mpsc, Arc, Mutex};

use eframe::egui;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use crate::config::{self, AgentStatus, Config};

// ─── Screen-position constants ────────────────────────────────────────────────

/// Window position when "hidden": far off every physical monitor.
/// We never use SW_HIDE because that suspends eframe's render loop.
/// WS_EX_TOOLWINDOW keeps this off-screen ghost out of Alt-Tab.
const POS_HIDDEN:  egui::Pos2 = egui::pos2(-32000.0, -32000.0);
/// Window position when the user opens the settings/password UI.
const POS_VISIBLE: egui::Pos2 = egui::pos2(200.0, 120.0);

// ─── Internal event type ──────────────────────────────────────────────────────

enum TrayMsg {
    /// Double-click on icon or "Settings" menu item.
    Show,
    /// "Exit" menu item – routes through the password prompt.
    Quit,
}

// ─── What to do after a successful password unlock ───────────────────────────

#[derive(Debug, Default, PartialEq, Clone)]
enum PendingAction {
    #[default]
    OpenSettings,
    Quit,
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
    view:           View,
    pending_action: PendingAction,
    /// Set by `open_window()` so `update()` calls `win32_show_window` this frame.
    bring_to_front: bool,

    _tray:   TrayIcon,
    tray_rx: mpsc::Receiver<TrayMsg>,

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
    is_quitting: bool,

    /// Win32 HWND stored as isize so the struct is Send-compatible.
    /// Populated on the first successful `FindWindowW` call in `update()`.
    hwnd: Option<isize>,

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
        // tray-icon's set_event_handler is the sole consumer of events.
        // We forward them into an mpsc channel and call request_repaint() so
        // update() wakes up immediately even when the window is SW_HIDE'd.
        let (tx, tray_rx) = mpsc::channel::<TrayMsg>();

        {
            let tx  = tx.clone();
            let ctx = cc.egui_ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                // Double-click left button → open password prompt
                if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = e {
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
            view:           View::Hidden,
            pending_action: PendingAction::default(),
            bring_to_front: false,
            _tray:          tray,
            tray_rx,
            icon_green,
            icon_yellow,
            icon_red,
            last_status:       AgentStatus::Disconnected,
            f_server_url:      initial_config.server_url.clone(),
            f_agent_name:      initial_config.agent_name.clone(),
            f_agent_password:  initial_config.agent_password.clone(),
            saved_config:      initial_config,
            f_new_ui_pass:     String::new(),
            f_confirm_ui_pass: String::new(),
            p_input:           String::new(),
            p_error:           false,
            p_focus_requested: false,
            form_msg:          None,
            is_quitting:       false,
            hwnd:              None,
            config_tx,
            agent_status,
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn handle_tray_events(&mut self) {
        while let Ok(msg) = self.tray_rx.try_recv() {
            match msg {
                TrayMsg::Show => {
                    self.pending_action = PendingAction::OpenSettings;
                    self.open_window();
                }
                TrayMsg::Quit => {
                    self.pending_action = PendingAction::Quit;
                    self.open_window();
                }
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

    /// Show the password prompt (or bring window to front if already visible).
    fn open_window(&mut self) {
        if self.view == View::Hidden {
            self.p_input.clear();
            self.p_error           = false;
            self.p_focus_requested = false;
            self.view              = View::PasswordPrompt;
        }
        // Always schedule a bring-to-front so the window grabs focus.
        self.bring_to_front = true;
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

        let new_cfg = Config {
            server_url:       self.f_server_url.trim().to_string(),
            agent_name:       self.f_agent_name.trim().to_string(),
            agent_password:   self.f_agent_password.clone(),
            ui_password_hash: ui_hash,
        };
        match config::save_config(&new_cfg) {
            Ok(_) => {
                self.saved_config = new_cfg.clone();
                let _ = self.config_tx.send(Some(new_cfg));
                self.f_new_ui_pass.clear();
                self.f_confirm_ui_pass.clear();
                self.form_msg = Some(("✓  Settings saved".into(), false));
            }
            Err(e) => {
                self.form_msg = Some((format!("Save failed: {e}"), true));
            }
        }
    }
}

// ─── eframe::App ─────────────────────────────────────────────────────────────

impl eframe::App for AgentApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── 1. Apply WS_EX_TOOLWINDOW once ───────────────────────────────
        //
        // We locate the HWND via FindWindowW (eframe 0.33 does not expose it
        // publicly).  Retry every frame until FindWindowW succeeds — the window
        // may not be fully created on the very first update() call.
        //
        // WS_EX_TOOLWINDOW hides the window from the taskbar and Alt-Tab, so
        // the "parked at -32000,-32000" trick is truly invisible to the user.
        // We never call SW_HIDE because that suspends eframe's render loop.
        #[cfg(target_os = "windows")]
        if self.hwnd.is_none() {
            if let Some(h) = win32_find_and_style_window() {
                self.hwnd = Some(h);
            }
        }

        let was_hidden = self.view == View::Hidden;

        // ── 2. Close button → hide (not quit), unless we are really quitting
        if ctx.input(|i| i.viewport().close_requested()) && !self.is_quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_window();
        }

        // ── 3. Drain tray / menu events (may flip self.view / bring_to_front)
        self.handle_tray_events();
        self.sync_tray_status();

        // ── 4. Render ─────────────────────────────────────────────────────
        match self.view {
            // Nothing to show — render a blank panel so egui clears the FB.
            View::Hidden => {
                egui::CentralPanel::default().show(ctx, |_ui| {});
            }

            // ── Password prompt ───────────────────────────────────────────
            View::PasswordPrompt => {
                let saved_hash = self.saved_config.ui_password_hash.clone();
                let mut intent: Option<bool> = None;

                let (heading, subtitle, btn_label) = match self.pending_action {
                    PendingAction::OpenSettings => (
                        "🔒  Agent Settings",
                        "Enter the UI access password:",
                        "🔓  Unlock",
                    ),
                    PendingAction::Quit => (
                        "🔒  Confirm Exit",
                        "Enter password to exit the agent:",
                        "✓  Exit Agent",
                    ),
                };

                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.heading(heading);
                        ui.add_space(10.0);
                        ui.label(subtitle);
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
                            .add_sized([140.0, 32.0], egui::Button::new(btn_label))
                            .clicked();

                        if (enter || clicked) && intent.is_none() {
                            intent = Some(
                                config::hash_password(&self.p_input) == saved_hash,
                            );
                        }

                        ui.add_space(8.0);
                        if ui
                            .add_sized([140.0, 28.0], egui::Button::new("Cancel"))
                            .clicked()
                        {
                            self.close_window();
                        }
                    });
                });

                match intent {
                    Some(true) => {
                        self.p_input.clear();
                        self.p_error = false;
                        match self.pending_action {
                            PendingAction::OpenSettings => {
                                self.f_server_url     = self.saved_config.server_url.clone();
                                self.f_agent_name     = self.saved_config.agent_name.clone();
                                self.f_agent_password = self.saved_config.agent_password.clone();
                                self.f_new_ui_pass.clear();
                                self.f_confirm_ui_pass.clear();
                                self.form_msg = None;
                                self.view = View::Settings;
                            }
                            PendingAction::Quit => {
                                self.is_quitting = true;
                            }
                        }
                    }
                    Some(false) => {
                        self.p_error = true;
                        self.p_input.clear();
                    }
                    None => {}
                }
            }

            // ── Settings panel ────────────────────────────────────────────
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
                                        .password(true)
                                        .hint_text("(blank = none)")
                                        .desired_width(290.0));
                                    ui.end_row();
                                });
                        });

                        ui.add_space(8.0);

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.label(egui::RichText::new("UI Access Password").strong());
                            ui.add_space(4.0);
                            egui::Grid::new("uipw").num_columns(2).spacing([8.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label("New Password:");
                                    ui.add(egui::TextEdit::singleline(&mut self.f_new_ui_pass)
                                        .password(true)
                                        .hint_text("leave blank to keep current")
                                        .desired_width(240.0));
                                    ui.end_row();

                                    ui.label("Confirm:");
                                    ui.add(egui::TextEdit::singleline(&mut self.f_confirm_ui_pass)
                                        .password(true)
                                        .desired_width(240.0));
                                    ui.end_row();
                                });
                        });

                        ui.add_space(8.0);

                        // Status badge
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            let (dot, label) = match &status {
                                AgentStatus::Connected    =>
                                    ("🟢", "Connected".to_string()),
                                AgentStatus::Connecting   =>
                                    ("🟡", "Connecting…".to_string()),
                                AgentStatus::Disconnected =>
                                    ("🔴", "Disconnected".to_string()),
                                AgentStatus::Error(e)     =>
                                    ("🔴", format!("Error: {e}")),
                            };
                            ui.horizontal(|ui| {
                                ui.label(dot);
                                ui.label(egui::RichText::new(label).monospace());
                            });
                        });

                        ui.add_space(12.0);

                        if let Some((msg, is_err)) = &self.form_msg {
                            let color = if *is_err {
                                egui::Color32::from_rgb(231, 76, 60)
                            } else {
                                egui::Color32::from_rgb(46, 204, 113)
                            };
                            ui.colored_label(color, msg);
                            ui.add_space(6.0);
                        }

                        ui.horizontal(|ui| {
                            if ui.add_sized([120.0, 32.0],
                                egui::Button::new("💾  Save")).clicked()
                            {
                                action = Some(SettingsAction::Save);
                            }
                            if ui.add_sized([100.0, 32.0],
                                egui::Button::new("✖  Close")).clicked()
                            {
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

        // ── 5. Position window (park off-screen or move on-screen) ───────────
        //
        // We NEVER call SW_HIDE — that suspends eframe's render loop and makes
        // tray events unprocessable.  Instead we park the window far off every
        // monitor while it is "hidden", and move it back when shown.
        // WS_EX_TOOLWINDOW (applied above) keeps the off-screen window out of
        // Alt-Tab so the user never stumbles across it accidentally.

        let now_hidden = self.view == View::Hidden;

        if now_hidden {
            // Keep the window parked off-screen every frame.
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(POS_HIDDEN));
        } else if self.bring_to_front {
            // Move on-screen and steal focus.
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(POS_VISIBLE));
            #[cfg(target_os = "windows")]
            if let Some(h) = self.hwnd {
                win32_focus_window(h);
            }
            self.bring_to_front = false;
        }

        if self.is_quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Tick at 10 Hz when hidden (enough for tray events), 60 Hz when visible.
        ctx.request_repaint_after(std::time::Duration::from_millis(
            if now_hidden { 100 } else { 16 },
        ));
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum SettingsAction { Save, Close }

// ── Win32 window management ──────────────────────────────────────────────────

/// Find the application window by its title, apply `WS_EX_TOOLWINDOW` (hide
/// from taskbar / Alt-Tab), and return the HWND as an `isize`.
///
/// Returns `None` if the window cannot be found yet (retry next frame).
#[cfg(target_os = "windows")]
fn win32_find_and_style_window() -> Option<isize> {
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongW, SetWindowLongW,
        GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    // Null-terminated UTF-16 window title
    let title: Vec<u16> = "Agent Settings\0".encode_utf16().collect();
    unsafe {
        let result = FindWindowW(
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR(title.as_ptr()),
        );
        match result {
            Ok(hwnd) if !hwnd.is_invalid() => {
                // Remove WS_EX_APPWINDOW (taskbar button), add WS_EX_TOOLWINDOW
                // (tool-style window: no taskbar entry, hidden from Alt-Tab).
                let ex  = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                let new = (ex | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0;
                SetWindowLongW(hwnd, GWL_EXSTYLE, new as i32);
                Some(hwnd.0 as usize as isize)
            }
            _ => None,
        }
    }
}

/// Bring the window to the foreground and give it keyboard focus.
///
/// Called after `ViewportCommand::OuterPosition(POS_VISIBLE)` has already
/// moved the window on-screen.  The double call (SetForegroundWindow +
/// BringWindowToTop) is the standard Windows trick to reliably steal focus
/// when triggered by a user action such as a tray icon click.
#[cfg(target_os = "windows")]
fn win32_focus_window(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{BringWindowToTop, SetForegroundWindow};
    unsafe {
        let h = HWND(hwnd as usize as *mut _);
        let _ = SetForegroundWindow(h);
        let _ = BringWindowToTop(h);
    }
}

// ── Tray icon generator ───────────────────────────────────────────────────────

/// Generate a 32×32 RGBA filled circle for use as a tray icon.
pub fn make_circle_icon(r: u8, g: u8, b: u8) -> tray_icon::Icon {
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
