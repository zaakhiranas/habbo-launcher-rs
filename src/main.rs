#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod client;
mod config;
mod login_code;
mod protocol;

use config::{Settings, UpdateSource, LAUNCHER_VERSION};
use login_code::LoginCode;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

const PINK: egui::Color32 = egui::Color32::from_rgb(0xE8, 0x5A, 0xA8);
const GREEN: egui::Color32 = egui::Color32::from_rgb(0x5A, 0xD1, 0x8B);
const RED: egui::Color32 = egui::Color32::from_rgb(0xE0, 0x5C, 0x5C);
const MUTED: egui::Color32 = egui::Color32::from_rgb(0x9A, 0x9A, 0xA5);

enum Msg {
    Status(String),
    Progress(u8),
    Done(String),
    Failed(String),
    Login(Option<LoginCode>),
}

struct Launcher {
    settings: Settings,
    login: Option<LoginCode>,
    last_ticket: Option<String>,
    status: String,
    progress: u8,
    busy: bool,
    log: Vec<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    show_settings: bool,
    show_log: bool,
    #[allow(dead_code)]
    protocol_ok: bool,
    /// Manually pasted login code (from the "Play with Habbo app" dialog).
    code_input: String,
    code_error: Option<String>,
    /// Ticket already handed to a client; tickets are single-use, so ignore it afterwards.
    used_ticket: Option<String>,
}

fn clipboard_thread(tx: Sender<Msg>, seed: bool) {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(_) => return,
    };
    // When launched from a `habbo://` link, whatever is already on the clipboard is usually
    // a code from an earlier attempt — and tickets are single-use, so only *new* copies
    // (i.e. the user clicking COPY on Habbo) are accepted.
    let mut last_seen = if seed {
        clipboard.get_text().unwrap_or_default()
    } else {
        String::new()
    };
    loop {
        if let Ok(text) = clipboard.get_text() {
            if text != last_seen {
                last_seen = text.clone();
                if let Some(code) = LoginCode::parse(&text) {
                    let _ = tx.send(Msg::Login(Some(code)));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

impl Launcher {
    fn new(cc: &eframe::CreationContext<'_>, protocol_arg: Option<&str>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx.style_mut(|style| {
            style.visuals.panel_fill = egui::Color32::from_rgb(0x1B, 0x1B, 0x21);
            style.visuals.window_fill = egui::Color32::from_rgb(0x23, 0x23, 0x2B);
            style.spacing.button_padding = egui::vec2(14.0, 8.0);
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        });

        let (tx, rx) = channel();
        let clip_tx = tx.clone();
        let started_from_link = protocol_arg.is_some();
        std::thread::spawn(move || clipboard_thread(clip_tx, started_from_link));

        let protocol_ok = protocol::register_habbo_protocol().is_ok();

        let mut log = vec![if protocol_ok {
            "habbo:// protocol registered".to_string()
        } else {
            "warning: could not register the habbo:// protocol".to_string()
        }];

        // Started from a `habbo://` link: adopt the login code if the link carries one, then
        // WAIT for the user to pick a client — no silent auto-launch.
        let parsed = protocol_arg.and_then(LoginCode::parse);
        if let Some(code) = &parsed {
            log.push(format!(
                "login code from habbo:// link [{}]",
                code.hotel_tag()
            ));
        }

        Launcher {
            settings: Settings::load(),
            last_ticket: parsed.as_ref().map(|c| c.sso_ticket.clone()),
            status: if parsed.is_some() {
                "Login code from Habbo — choose a client below".to_string()
            } else if started_from_link {
                "Opened from Habbo — click COPY there, then choose a client".to_string()
            } else {
                "Waiting for a login code on the clipboard".to_string()
            },
            login: parsed,
            progress: 0,
            busy: false,
            log,
            tx,
            rx,
            // `HCL_OPEN_SETTINGS=1` opens the settings panel at startup (UI smoke test)
            show_settings: std::env::var("HCL_OPEN_SETTINGS").is_ok(),
            show_log: false,
            protocol_ok,
            code_input: String::new(),
            code_error: None,
            used_ticket: None,
        }
    }

    fn push_log(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    /// Adopt a login code pasted from the "Play with Habbo app" dialog.
    fn apply_manual_code(&mut self) {
        let raw = self.code_input.trim().to_string();
        if raw.is_empty() {
            self.code_error = Some("Paste the login code first".into());
            return;
        }
        match LoginCode::parse(&raw) {
            Some(code) => {
                let tag = code.hotel_tag();
                self.push_log(format!("login code entered [{tag}]"));
                self.last_ticket = Some(code.sso_ticket.clone());
                if !self.busy {
                    self.status = format!("Ready — login code entered [{tag}]");
                }
                self.login = Some(code);
                self.code_error = None;
                self.code_input.clear();
            }
            None => {
                self.code_error =
                    Some("Not a Habbo login code — expected e.g. hhus.84566da9-…-63384688".into());
            }
        }
    }

    /// Pull a fresh code straight from the clipboard and adopt it.
    fn apply_clipboard_code(&mut self) {
        match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(text) => {
                self.code_input = text;
                self.apply_manual_code();
            }
            Err(_) => self.code_error = Some("Clipboard is empty or unreadable".into()),
        }
    }

    fn start_flow(&mut self, source: UpdateSource) {
        let Some(login) = self.login.clone() else {
            self.status = "No login code detected yet — paste a code or copy one from Habbo".into();
            return;
        };
        let mut settings = self.settings.clone();
        settings.update_source = source;
        let tx = self.tx.clone();
        self.busy = true;
        self.progress = 0;
        self.status = "Checking for client updates".into();
        self.push_log(format!(
            "flow start: source={} hotel={} user={}",
            settings.update_source.as_str(),
            login.server_id,
            if login.username.is_empty() {
                "?"
            } else {
                &login.username
            }
        ));

        std::thread::spawn(move || {
            let rep = |pct: u8, msg: &str| {
                let _ = tx.send(Msg::Progress(pct));
                if !msg.is_empty() {
                    let _ = tx.send(Msg::Status(msg.to_string()));
                }
            };
            let result = (|| -> Result<(), String> {
                let remote =
                    client::fetch_remote_client(settings.update_source, &login.server_url, &rep)?;
                if client::is_client_version_exists(settings.update_source, &remote.version) {
                    rep(93, "Client already up to date");
                } else {
                    client::update_client(&settings, &remote, &rep)?;
                }
                client::launch_client(&settings, &remote.version, &login, &rep)?;
                Ok(())
            })();
            let _ = match result {
                Ok(()) => tx.send(Msg::Done("Client launched".to_string())),
                Err(e) => tx.send(Msg::Failed(e)),
            };
        });
    }

    fn start_instance_only(&mut self) {
        let Some(login) = self.login.clone() else {
            self.status = "No login code detected yet".into();
            return;
        };
        let settings = self.settings.clone();
        let tx = self.tx.clone();
        self.busy = true;
        std::thread::spawn(move || {
            let rep = |pct: u8, msg: &str| {
                let _ = tx.send(Msg::Progress(pct));
                if !msg.is_empty() {
                    let _ = tx.send(Msg::Status(msg.to_string()));
                }
            };
            let version = client_root(&settings)
                .map(|d| d.join("VERSION.txt"))
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default()
                .trim()
                .to_string();
            let result = if version.is_empty() {
                Err("No installed client found — run a full update first".to_string())
            } else {
                client::launch_client(&settings, &version, &login, &rep)
            };
            let _ = match result {
                Ok(()) => tx.send(Msg::Done("New instance launched".to_string())),
                Err(e) => tx.send(Msg::Failed(e)),
            };
        });
    }

    fn drain_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Status(s) => {
                    self.push_log(s.clone());
                    self.status = s;
                }
                Msg::Progress(p) => self.progress = p,
                Msg::Done(s) => {
                    self.busy = false;
                    self.progress = 100;
                    self.status = s.clone();
                    self.push_log(s);
                    // The ticket went to a client; it can never be used again.
                    if let Some(code) = self.login.take() {
                        self.used_ticket = Some(code.sso_ticket);
                    }
                    self.last_ticket = None;
                }
                Msg::Failed(e) => {
                    self.busy = false;
                    self.progress = 0;
                    self.status = format!("error: {e}");
                    self.push_log(format!("error: {e}"));
                }
                Msg::Login(code) => {
                    if let Some(code) = code {
                        // A ticket that already went to a client is dead (single use); ignore
                        // it so the clipboard can't resurrect a spent code.
                        if self.used_ticket.as_ref() == Some(&code.sso_ticket) {
                            continue;
                        }
                        if Some(&code.sso_ticket) != self.last_ticket.as_ref() {
                            self.push_log(format!("login code detected [{}]", code.hotel_tag()));
                            self.last_ticket = Some(code.sso_ticket.clone());
                        }
                        let tag = code.hotel_tag();
                        if !self.busy {
                            self.status = format!("Ready — login code detected [{tag}]");
                        }
                        self.login = Some(code);
                        ctx.request_repaint();
                    }
                    // a clipboard holding unrelated text never clears a valid session
                }
            }
        }
    }

    fn footer_text(&self) -> String {
        match &self.login {
            Some(code) if !code.username.is_empty() => {
                let mut name = code.username.clone();
                if name.chars().count() > 15 {
                    name = name.chars().take(15).collect::<String>() + "...";
                }
                format!("Playing as {name}")
            }
            _ => format!("Launcher version {LAUNCHER_VERSION}"),
        }
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let mut changed = false;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Update source");
                for source in [UpdateSource::AirPlus, UpdateSource::AirOfficial] {
                    if ui
                        .selectable_label(self.settings.update_source == source, source.label())
                        .clicked()
                    {
                        self.settings.update_source = source;
                        changed = true;
                    }
                }
                ui.add_space(6.0);
                ui.label("Launcher scale");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut self.settings.window_scale, 0.75..=1.5)
                            .fixed_decimals(2)
                            .suffix("x"),
                    )
                    .changed();

                ui.add_space(6.0);
                ui.label("Client render mode");
                egui::ComboBox::from_id_salt("render_mode")
                    .selected_text(self.settings.client_render_mode.to_uppercase())
                    .show_ui(ui, |ui| {
                        for mode in ["cpu", "direct", "gpu"] {
                            if ui
                                .selectable_value(
                                    &mut self.settings.client_render_mode,
                                    mode.to_string(),
                                    mode.to_uppercase(),
                                )
                                .clicked()
                            {
                                changed = true;
                            }
                        }
                    });

                ui.label("Client resolution");
                egui::ComboBox::from_id_salt("resolution")
                    .selected_text(self.settings.client_resolution.to_uppercase())
                    .show_ui(ui, |ui| {
                        for res in ["standard", "high"] {
                            if ui
                                .selectable_value(
                                    &mut self.settings.client_resolution,
                                    res.to_string(),
                                    res.to_uppercase(),
                                )
                                .clicked()
                            {
                                changed = true;
                            }
                        }
                    });

                if cfg!(target_os = "macos") {
                    ui.label("AIR runtime version");
                    egui::ComboBox::from_id_salt("air_version")
                        .selected_text(if self.settings.client_air_version == "old" {
                            "AIR 50"
                        } else {
                            "AIR 51"
                        })
                        .show_ui(ui, |ui| {
                            for (value, label) in [("latest", "AIR 51"), ("old", "AIR 50")] {
                                if ui
                                    .selectable_value(
                                        &mut self.settings.client_air_version,
                                        value.to_string(),
                                        label,
                                    )
                                    .clicked()
                                {
                                    changed = true;
                                }
                            }
                        });
                }

                ui.add_space(8.0);
                if ui.button("Reset to defaults").clicked() {
                    self.settings = Settings::default();
                    changed = true;
                }
                ui.label(
                    egui::RichText::new(format!(
                        "Settings file: {}",
                        config::settings_path().display()
                    ))
                    .small()
                    .color(MUTED),
                );
            });
        self.show_settings = open;
        if changed {
            match self.settings.save() {
                Ok(()) => self.push_log("settings saved"),
                Err(e) => self.push_log(format!("could not save settings: {e}")),
            }
        }
    }
}

/// Directory holding the newest installed build of the active source.
fn client_root(settings: &Settings) -> Option<std::path::PathBuf> {
    let kind = match settings.update_source {
        UpdateSource::AirPlus => "airplus",
        UpdateSource::AirOfficial => "air",
    };
    let root = config::downloads_dir().join(kind);
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(&root).ok()?.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(meta) = std::fs::metadata(dir.join("HabboAir.swf")) else {
            continue;
        };
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
            newest = Some((modified, dir));
        }
    }
    newest.map(|(_, d)| d)
}

impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_messages(ctx);

        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        let scale = self.settings.window_scale.clamp(0.75, 1.5);
        if (ctx.pixels_per_point() - scale).abs() > 0.01 {
            ctx.set_pixels_per_point(scale);
        }

        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(self.footer_text()).small().color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Log").clicked() {
                        self.show_log = !self.show_log;
                    }
                    if ui.small_button("Settings").clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    if ui.small_button("Website").clicked() {
                        let url = match &self.login {
                            Some(code) => format!("https://{}/settings/avatars", code.server_url),
                            None => "https://www.habbo.com/settings/avatars".to_string(),
                        };
                        let _ = webbrowser::open(&url);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);
            ui.heading(egui::RichText::new("Habbo Custom Launcher").color(PINK));
            ui.label(
                egui::RichText::new("Rust port · AIR / AIR+ classic client")
                    .small()
                    .color(MUTED),
            );
            ui.add_space(10.0);

            match &self.login {
                Some(code) => {
                    let user = if code.username.is_empty() {
                        "unknown".to_string()
                    } else {
                        code.username.clone()
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "Login code detected [{}] · {user}",
                            code.hotel_tag()
                        ))
                        .color(GREEN)
                        .strong(),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new("No login code yet")
                            .color(RED)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Paste the code from Habbo's \"Play with Habbo app\" dialog, copy one from the hotel settings page, or open a habbo:// link.",
                        )
                        .small()
                        .color(MUTED),
                    );
                }
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let edit_width = (ui.available_width() - 178.0).max(110.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.code_input)
                        .hint_text("Paste login code  ·  hhus.8456…-63384688")
                        .desired_width(edit_width),
                );
                let entered =
                    resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Use code").clicked() || entered {
                    self.apply_manual_code();
                }
                if ui.button("Paste").clicked() {
                    self.apply_clipboard_code();
                }
            });
            ui.label(
                egui::RichText::new("Login codes are single-use — paste a fresh one right before launching.")
                    .small()
                    .color(MUTED),
            );
            if let Some(err) = self.code_error.clone() {
                ui.label(egui::RichText::new(err).small().color(RED));
            }

            ui.add_space(12.0);
            let can_launch = self.login.is_some() && !self.busy;
            let button = |ui: &mut egui::Ui, text: &str, height: f32| {
                ui.add_enabled(
                    can_launch,
                    egui::Button::new(egui::RichText::new(text).size(15.0))
                        .min_size(egui::vec2(ui.available_width(), height)),
                )
                .clicked()
            };

            if button(ui, "Launch official client (AIR)", 40.0) {
                self.start_flow(UpdateSource::AirOfficial);
            }
            if button(ui, "Launch AIR+ (unofficial client) · Lilith release", 40.0) {
                self.start_flow(UpdateSource::AirPlus);
            }
            if button(ui, "Start another instance", 26.0) {
                self.start_instance_only();
            }

            ui.add_space(10.0);
            if self.busy || self.progress > 0 {
                ui.add(
                    egui::ProgressBar::new(self.progress as f32 / 100.0)
                        .show_percentage()
                        .animate(self.busy),
                );
            }
            ui.label(egui::RichText::new(&self.status).small().color(MUTED));

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Update source:").small().color(MUTED));
                if ui
                    .selectable_label(false, self.settings.update_source.label())
                    .on_hover_text("Click to switch between the official client and AIR+")
                    .clicked()
                {
                    self.settings.update_source = match self.settings.update_source {
                        UpdateSource::AirPlus => UpdateSource::AirOfficial,
                        UpdateSource::AirOfficial => UpdateSource::AirPlus,
                    };
                    let _ = self.settings.save();
                }
            });

            if self.show_log {
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(120.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.log {
                            ui.label(egui::RichText::new(line).monospace().small().color(MUTED));
                        }
                    });
            }
        });

        self.settings_ui(ctx);
    }
}

/// Headless end-to-end check: resolves, downloads, extracts and patches a client without launching it.
fn selftest(args: &[String]) -> Result<(), String> {
    let mut settings = Settings::load();
    settings.update_source = if args.iter().any(|a| a == "official") {
        UpdateSource::AirOfficial
    } else {
        UpdateSource::AirPlus
    };
    let rep = |pct: u8, msg: &str| println!("[{pct:3}%] {msg}");
    let remote = client::fetch_remote_client(settings.update_source, "www.habbo.com", &rep)?;
    println!("remote client version: {}", remote.version);
    println!("resolved url: {}", remote.url);
    if args.iter().any(|a| a == "check-only") {
        println!("check-only: stopping before download");
        return Ok(());
    }
    println!(
        "already installed: {}",
        client::is_client_version_exists(settings.update_source, &remote.version)
    );
    client::update_client(&settings, &remote, &rep)?;

    let dir = config::client_dir(settings.update_source, &remote.version);
    println!("client dir: {}", dir.display());
    let mut ok = true;
    for name in [
        "HabboAir.swf",
        "Habbo.exe",
        "META-INF/AIR/application.xml",
        "AIR_HASH.txt",
    ] {
        let present = dir.join(name).is_file();
        println!("  {name}: {}", if present { "ok" } else { "MISSING" });
        ok &= present;
    }
    let swf = std::fs::read(dir.join("HabboAir.swf")).map_err(|e| e.to_string())?;
    let swf_version = swf.get(3).copied().unwrap_or(0);
    println!("  swf version byte: {swf_version}");
    ok &= swf_version == 51;
    let xml = std::fs::read_to_string(dir.join("META-INF/AIR/application.xml"))
        .map_err(|e| e.to_string())?;
    for needle in ["<renderMode>cpu</renderMode>", "requestedDisplayResolution"] {
        let found = xml.contains(needle);
        println!("  application.xml contains {needle}: {found}");
        ok &= found;
    }
    // launch is exercised by the GUI flow; here we only prove the exe exists
    ok &= client::client_executable(&dir).is_file();
    if ok {
        println!("selftest OK");
        Ok(())
    } else {
        Err("one or more checks failed".into())
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--selftest") {
        match selftest(&args) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("selftest failed: {e}");
                std::process::exit(1);
            }
        }
    }

    // `habbo://` protocol launches carry the login code as an argument.
    // Browsers/Windows may hand it over with different casing or wrapped in quotes.
    let protocol_arg = std::env::args()
        .map(|a| a.trim_matches('"').to_string())
        .find(|a| a.to_ascii_lowercase().starts_with("habbo://"));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 520.0])
            .with_min_inner_size([440.0, 460.0])
            .with_title("Habbo Custom Launcher"),
        ..Default::default()
    };

    eframe::run_native(
        "Habbo Custom Launcher",
        options,
        Box::new(move |cc| {
            let app = Launcher::new(cc, protocol_arg.as_deref());
            Ok(Box::new(app))
        }),
    )
}
