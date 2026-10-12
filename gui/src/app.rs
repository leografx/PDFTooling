//! The settings window: sidebar of sections, grouped rows of settings,
//! toolbar, status bar, TOML preview and Mac-style alert sheets.

use crate::doc;
use crate::schema::{self, Field, FileKind, Look, Section, Ty, Unit};
use crate::theme::{self, Pal};
use eframe::egui::{
    self, vec2, Align, Color32, CornerRadius, FontFamily, FontId, Key, KeyboardShortcut, Layout, Margin, Modifiers,
    RichText, Sense, Stroke, Ui, ViewportCommand,
};
use egui_phosphor::regular as I;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

// ================================================================== document

/// Result of checking the file with the real engine's parser.
#[derive(Clone, Default)]
pub struct Check {
    pub ok: bool,
    pub message: String,
    /// Schema key the problem is about, when known.
    pub key: Option<&'static str>,
    pub notes: Vec<String>,
}

pub struct Open {
    pub kind: FileKind,
    pub path: Option<PathBuf>,
    pub doc: DocumentMut,
    saved: String,
    cur: String,
    undo: Vec<String>,
    redo: Vec<String>,
    last_edit: f64,
    pub check: Check,
    map_buf: Option<String>,
    map_err: Option<String>,
}

struct Eff<'a> {
    kind: FileKind,
    doc: &'a DocumentMut,
}

fn def_str(kind: FileKind, key: &str) -> String {
    match schema::find(kind, key).map(|f| f.ty) {
        Some(Ty::Choice { def, .. } | Ty::Text { def, .. } | Ty::PdfFile { def }) => def.to_string(),
        _ => String::new(),
    }
}
fn def_num(kind: FileKind, key: &str) -> f64 {
    match schema::find(kind, key).map(|f| f.ty) {
        Some(Ty::Num { def, .. }) => def,
        Some(Ty::Int { def, .. } | Ty::IntChoice { def, .. }) => def as f64,
        _ => 0.0,
    }
}
fn def_bool(kind: FileKind, key: &str) -> bool {
    matches!(schema::find(kind, key).map(|f| f.ty), Some(Ty::Bool(true)))
}

impl Look for Eff<'_> {
    fn s(&self, key: &str) -> String {
        doc::get_str(self.doc, key).unwrap_or_else(|| def_str(self.kind, key))
    }
    fn n(&self, key: &str) -> f64 {
        doc::get_f64(self.doc, key).unwrap_or_else(|| def_num(self.kind, key))
    }
    fn b(&self, key: &str) -> bool {
        doc::get_bool(self.doc, key).unwrap_or_else(|| def_bool(self.kind, key))
    }
}

/// Validate settings text with the engine's own parser and rules.
pub fn check(kind: FileKind, text: &str) -> Check {
    let all_keys: Vec<&'static str> = {
        let mut v: Vec<&'static str> = kind.sections().iter().flat_map(|s| s.fields()).map(|f| f.key).collect();
        v.sort_by_key(|k| std::cmp::Reverse(k.len()));
        v
    };
    let fail = |msg: String| {
        let line_key = toml_error_key(text, &msg);
        let key = all_keys.iter().copied().find(|k| msg.contains(k)).or(line_key.and_then(|k| all_keys.iter().copied().find(|x| *x == k)));
        Check { ok: false, message: tidy_error(&msg), key, notes: vec![] }
    };
    let mut notes = Vec::new();
    match kind {
        FileKind::Job => {
            let j: pdf_impose::sheet::config::Job = match toml::from_str(text) {
                Ok(j) => j,
                Err(e) => return fail(e.to_string()),
            };
            if let Err(e) = j.validate() {
                return fail(format!("{e:#}"));
            }
            if let Some(m) = &j.pages.map {
                if let Err(e) = pdf_impose::sheet::layout::parse_map(m) {
                    return fail(format!("pages.map: {e:#}"));
                }
            }
            use pdf_impose::sheet::config::Folio;
            if j.binding.lip > 0.0 && j.binding.lip_side == Folio::None {
                notes.push("Lip is set but “Lip on” is None — no lip will be added.".into());
            }
            if j.layout.one_signature_per_sheet && !j.binding.style.folded() {
                notes.push("“One signature per sheet” only applies to saddle and perfect binding.".into());
            }
        }
        FileKind::Impose => {
            let c: pdf_impose::row::config::Config = match toml::from_str(text) {
                Ok(c) => c,
                Err(e) => return fail(e.to_string()),
            };
            if let Err(e) = c.validate() {
                return fail(format!("{e:#}"));
            }
            if c.mark.spot.enabled && c.mark.spot.name != c.mark.spot.name.trim() {
                notes.push("The spot name has spaces at the start or end.".into());
            }
        }
    }
    Check { ok: true, message: String::new(), key: None, notes }
}

/// Shorten a multi-line TOML error to "line N: message".
fn tidy_error(msg: &str) -> String {
    let lines: Vec<&str> = msg.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.len() > 1 && lines[0].starts_with("TOML parse error") {
        let at = lines[0].trim_start_matches("TOML parse error at ").replace(", column", " col");
        format!("{} ({at})", lines.last().unwrap_or(&""))
    } else {
        lines.join(" ")
    }
}

/// For "TOML parse error at line N" messages, the dotted key on that line.
fn toml_error_key(text: &str, msg: &str) -> Option<String> {
    let n: usize = msg.split("line ").nth(1)?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    let mut table = String::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.starts_with('[') {
            table = t.trim_matches(|c| c == '[' || c == ']').split(']').next().unwrap_or("").trim().to_string();
        }
        if i + 1 == n {
            let k = t.split('=').next()?.trim();
            return Some(if table.is_empty() { k.to_string() } else { format!("{table}.{k}") });
        }
    }
    None
}

impl Open {
    pub fn new(kind: FileKind, path: Option<PathBuf>, text: &str, saved: &str) -> anyhow::Result<Open> {
        let doc: DocumentMut = text.parse()?;
        let cur = doc.to_string();
        Ok(Open {
            kind,
            path,
            check: check(kind, &cur),
            doc,
            saved: saved.to_string(),
            cur,
            undo: vec![],
            redo: vec![],
            last_edit: 0.0,
            map_buf: None,
            map_err: None,
        })
    }

    pub fn dirty(&self) -> bool {
        self.cur != self.saved
    }

    fn units(&self) -> String {
        doc::get_str(&self.doc, "units").unwrap_or_else(|| "in".into())
    }

    fn eff(&self) -> Eff<'_> {
        Eff { kind: self.kind, doc: &self.doc }
    }

    /// Call once per frame after the UI: records undo steps and re-validates.
    fn commit(&mut self, now: f64) {
        let text = self.doc.to_string();
        if text != self.cur {
            if now - self.last_edit > 0.8 || self.undo.is_empty() {
                self.undo.push(self.cur.clone());
                if self.undo.len() > 200 {
                    self.undo.remove(0);
                }
            }
            self.redo.clear();
            self.last_edit = now;
            self.cur = text;
            self.check = check(self.kind, &self.cur);
        }
    }

    fn restore(&mut self, text: String) {
        if let Ok(d) = text.parse() {
            self.doc = d;
            self.cur = text;
            self.check = check(self.kind, &self.cur);
            self.map_buf = None;
            self.map_err = None;
            self.last_edit = 0.0;
        }
    }

    fn undo(&mut self) {
        if let Some(t) = self.undo.pop() {
            self.redo.push(self.cur.clone());
            self.restore(t);
        }
    }
    fn redo(&mut self) {
        if let Some(t) = self.redo.pop() {
            self.undo.push(self.cur.clone());
            self.restore(t);
        }
    }

    fn set_count(&self) -> (usize, usize) {
        let all: Vec<&Field> = self.kind.sections().iter().flat_map(|s| s.fields()).collect();
        (all.iter().filter(|f| doc::has(&self.doc, f.key)).count(), all.len())
    }

    /// Convert every length from `from` to `to` units, writing defaults that
    /// aren't in the file yet so their meaning stays the same.
    fn convert_units(&mut self, from: &str, to: &str) {
        let pt = |u: &str| match u {
            "mm" => 72.0 / 25.4,
            "pt" => 1.0,
            _ => 72.0,
        };
        let k = pt(from) / pt(to);
        let decimals = match to {
            "in" => 1e4,
            _ => 1e3,
        };
        let r = |v: f64| (v * k * decimals).round() / decimals;
        for f in self.kind.sections().iter().flat_map(|s| s.fields()) {
            match f.ty {
                Ty::Num { def, unit: Unit::Len, .. } => {
                    let v = doc::get_f64(&self.doc, f.key).unwrap_or(def);
                    if v != 0.0 || doc::has(&self.doc, f.key) {
                        doc::set_value(&mut self.doc, f.key, doc::num(r(v)));
                    }
                }
                Ty::OptNum { unit: Unit::Len, .. } => {
                    if let Some(v) = doc::get_f64(&self.doc, f.key) {
                        doc::set_value(&mut self.doc, f.key, doc::num(r(v)));
                    }
                }
                _ => {}
            }
        }
    }
}

// ================================================================== app state

enum After {
    Open(PathBuf),
    OpenDialog,
    New(FileKind, Option<PathBuf>),
    Close,
    Revert,
}

enum Modal {
    Unsaved(After),
    Error { title: String, msg: String },
    AskKind { path: PathBuf, text: String },
    NoSettings { dir: PathBuf },
    ConvertUnits { from: String, to: String },
    SaveInvalid { path: PathBuf, msg: String },
}

enum Event {
    Units { from: String, to: String },
}

pub struct App {
    open: Option<Open>,
    section: usize,
    search: String,
    show_toml: bool,
    modal: Option<Modal>,
    recent: Vec<PathBuf>,
    allow_close: bool,
    flash: Option<(&'static str, f64)>,
    status: Option<(String, f64)>,
    title: String,
    focus_search: bool,
    repaint: bool,
}

fn recent_file() -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
    };
    Some(base.join("impose-settings").join("recent.txt"))
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, arg: Option<PathBuf>) -> App {
        theme::install_fonts(&cc.egui_ctx);
        theme::install_style(&cc.egui_ctx);
        let recent = recent_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| s.lines().map(PathBuf::from).filter(|p| p.exists()).collect())
            .unwrap_or_default();
        let mut app = App {
            open: None,
            section: 0,
            search: String::new(),
            show_toml: false,
            modal: None,
            recent,
            allow_close: false,
            flash: None,
            status: None,
            title: String::new(),
            focus_search: false,
            repaint: false,
        };
        if let Some(p) = arg {
            app.open_path(&p);
        }
        // Appearance override: IMPOSE_SETTINGS_THEME=light | dark (default: follow the system).
        match std::env::var("IMPOSE_SETTINGS_THEME").as_deref() {
            Ok("light") => cc.egui_ctx.set_theme(egui::ThemePreference::Light),
            Ok("dark") => cc.egui_ctx.set_theme(egui::ThemePreference::Dark),
            _ => {}
        }
        // Start on a section (used for screenshots): IMPOSE_SETTINGS_SECTION=3
        if let Some(n) = std::env::var("IMPOSE_SETTINGS_SECTION").ok().and_then(|v| v.parse().ok()) {
            app.section = n;
        }
        app.show_toml = std::env::var("IMPOSE_SETTINGS_SHOW_TOML").is_ok();
        app
    }

    fn remember(&mut self, p: &Path) {
        let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        self.recent.retain(|x| x != &p);
        self.recent.insert(0, p);
        self.recent.truncate(8);
        if let Some(f) = recent_file() {
            let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
            let text: Vec<String> = self.recent.iter().map(|p| p.display().to_string()).collect();
            let _ = std::fs::write(f, text.join("\n"));
        }
    }

    fn say(&mut self, msg: impl Into<String>, now: f64) {
        self.status = Some((msg.into(), now));
    }

    /// Run `after` now, or ask to save first if there are unsaved changes.
    fn guard(&mut self, after: After) {
        if self.open.as_ref().is_some_and(|o| o.dirty()) {
            self.modal = Some(Modal::Unsaved(after));
        } else {
            self.perform(after);
        }
    }

    fn perform(&mut self, after: After) {
        match after {
            After::Open(p) => self.open_path(&p),
            After::OpenDialog => {
                let mut d = rfd::FileDialog::new().set_title("Open Settings").add_filter("Settings", &["toml"]);
                if let Some(dir) = self.open.as_ref().and_then(|o| o.path.as_ref()).and_then(|p| p.parent()) {
                    d = d.set_directory(dir);
                }
                if let Some(p) = d.pick_file() {
                    self.open_path(&p);
                }
            }
            After::New(kind, path) => {
                let t = kind.template();
                // a new file for a given path isn't on disk yet, so it starts as edited
                let saved = if path.is_some() { "" } else { t };
                match Open::new(kind, path, t, saved) {
                    Ok(o) => {
                        self.open = Some(o);
                        self.section = 0;
                    }
                    Err(e) => self.modal = Some(Modal::Error { title: "Couldn’t create the file".into(), msg: e.to_string() }),
                }
            }
            After::Close => {
                self.allow_close = true;
                self.repaint = true;
            }
            After::Revert => {
                if let Some(p) = self.open.as_ref().and_then(|o| o.path.clone()) {
                    self.open_path(&p);
                }
            }
        }
    }

    fn open_path(&mut self, p: &Path) {
        if p.is_dir() {
            let job = p.join("job.toml");
            let imp = p.join("impose.toml");
            if job.exists() {
                return self.open_path(&job);
            }
            if imp.exists() {
                return self.open_path(&imp);
            }
            self.modal = Some(Modal::NoSettings { dir: p.to_path_buf() });
            return;
        }
        let text = match std::fs::read_to_string(p) {
            Ok(t) => t,
            Err(e) => {
                self.modal = Some(Modal::Error { title: format!("Couldn’t open “{}”", file_name(p)), msg: e.to_string() });
                return;
            }
        };
        let d: DocumentMut = match text.parse() {
            Ok(d) => d,
            Err(e) => {
                self.modal = Some(Modal::Error {
                    title: format!("“{}” isn’t a valid TOML file", file_name(p)),
                    msg: tidy_error(&e.to_string()),
                });
                return;
            }
        };
        if d.get("hotfolder").is_some() {
            self.modal = Some(Modal::Error {
                title: "This is a watch list".into(),
                msg: format!(
                    "“{}” lists hot folders to watch. Open the impose.toml or job.toml inside one of those folders instead.",
                    file_name(p)
                ),
            });
            return;
        }
        match detect(p, &d) {
            Some(kind) => self.load(kind, p, &text),
            None => self.modal = Some(Modal::AskKind { path: p.to_path_buf(), text }),
        }
    }

    fn load(&mut self, kind: FileKind, p: &Path, text: &str) {
        match Open::new(kind, Some(p.to_path_buf()), text, text) {
            Ok(o) => {
                let keep = self.open.as_ref().map(|x| x.kind) == Some(kind);
                self.open = Some(o);
                if !keep {
                    self.section = 0;
                }
                self.remember(p);
            }
            Err(e) => self.modal = Some(Modal::Error { title: "Couldn’t open the file".into(), msg: e.to_string() }),
        }
    }

    fn save(&mut self, save_as: bool, now: f64, force: bool) {
        let Some(o) = self.open.as_ref() else { return };
        let path = match (&o.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let mut d = rfd::FileDialog::new().set_title("Save Settings").add_filter("Settings", &["toml"]).set_file_name(
                    o.path.as_ref().map(|p| file_name(p)).unwrap_or_else(|| o.kind.file_name().to_string()),
                );
                if let Some(dir) = o.path.as_ref().and_then(|p| p.parent()) {
                    d = d.set_directory(dir);
                }
                match d.save_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        if !o.check.ok && !force {
            self.modal = Some(Modal::SaveInvalid { path, msg: o.check.message.clone() });
            return;
        }
        let text = o.doc.to_string();
        let tmp = path.with_extension("toml.saving");
        let res = std::fs::write(&tmp, &text).and_then(|_| std::fs::rename(&tmp, &path));
        match res {
            Ok(()) => {
                let o = self.open.as_mut().unwrap();
                o.saved = text;
                o.path = Some(path.clone());
                self.remember(&path);
                self.say(format!("Saved {}", file_name(&path)), now);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                self.modal = Some(Modal::Error { title: format!("Couldn’t save “{}”", file_name(&path)), msg: e.to_string() });
            }
        }
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string())
}

/// Which kind of settings file this is, from its name, then its sections.
pub fn detect(p: &Path, d: &DocumentMut) -> Option<FileKind> {
    match p.file_name().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("job.toml") => return Some(FileKind::Job),
        Some("impose.toml") => return Some(FileKind::Impose),
        _ => {}
    }
    let job = ["press", "binding", "product", "colorbar", "pages", "marks"].iter().any(|k| d.contains_key(k));
    let imp = ["mark", "bleed"].iter().any(|k| d.contains_key(k));
    match (job, imp) {
        (true, false) => Some(FileKind::Job),
        (false, true) => Some(FileKind::Impose),
        _ => None,
    }
}

fn reveal(p: &Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(p.parent().unwrap_or(p)).spawn();
}

// ================================================================== frame

impl eframe::App for App {
    fn ui(&mut self, root: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let now = ctx.input(|i| i.time);
        let p = theme::pal(root);

        self.shortcuts(&ctx, now);
        self.drops(&ctx);

        // close with unsaved changes → ask
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.open.as_ref().is_some_and(|o| o.dirty()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if self.modal.is_none() {
                self.modal = Some(Modal::Unsaved(After::Close));
            }
        }
        if self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        let title = match &self.open {
            Some(o) => format!(
                "{}{} — Impose Settings",
                o.path.as_ref().map(|p| file_name(p)).unwrap_or_else(|| format!("Untitled {}", o.kind.file_name())),
                if o.dirty() { " — Edited" } else { "" }
            ),
            None => "Impose Settings".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::left("sidebar")
            .resizable(false)
            .exact_size(236.0)
            .show_separator_line(true)
            .frame(egui::Frame::new().fill(p.sidebar).inner_margin(Margin { left: 10, right: 10, top: 0, bottom: 10 }))
            .show(root, |ui| self.sidebar(ui, &p));

        egui::CentralPanel::no_frame().show(root, |ui| {
            egui::Panel::top("toolbar")
                .exact_size(52.0)
                .frame(egui::Frame::new().fill(p.content).inner_margin(Margin::symmetric(16, 0)))
                .show(ui, |ui| self.toolbar(ui, &p, now));
            egui::Panel::bottom("status")
                .exact_size(28.0)
                .frame(egui::Frame::new().fill(p.content).inner_margin(Margin::symmetric(16, 0)))
                .show(ui, |ui| self.status_bar(ui, &p, now));
            if self.show_toml && self.open.is_some() {
                egui::Panel::right("toml")
                    .resizable(true)
                    .default_size(380.0)
                    .size_range(260.0..=900.0)
                    .frame(egui::Frame::new().fill(if p.dark { Color32::from_rgb(0x1A, 0x1A, 0x1A) } else { Color32::from_rgb(0xFB, 0xFB, 0xFB) }).inner_margin(Margin::same(12)))
                    .show(ui, |ui| self.toml_view(ui, &p));
            }
            egui::CentralPanel::no_frame().show(ui, |ui| {
                ui.painter().rect_filled(ui.max_rect(), CornerRadius::ZERO, p.content);
                if self.open.is_some() {
                    self.content(ui, &p, now);
                } else {
                    self.welcome(ui, &p);
                }
            });
        });

        self.hover_overlay(&ctx, &p);
        self.modals(&ctx, &p, now);

        if let Some(o) = self.open.as_mut() {
            o.commit(now);
        }
        if std::mem::take(&mut self.repaint) {
            ctx.request_repaint();
        }
    }
}

impl App {
    fn shortcuts(&mut self, ctx: &egui::Context, now: f64) {
        let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        let cmd_shift = |k| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
        if self.modal.is_some() {
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd_shift(Key::S))) {
            self.save(true, now, false);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::S))) {
            self.save(false, now, false);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::O))) {
            self.guard(After::OpenDialog);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::N))) {
            self.guard(After::New(FileKind::Job, None));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::F))) {
            self.focus_search = true;
        }
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_shortcut(&cmd_shift(Key::Z))) {
                if let Some(o) = self.open.as_mut() {
                    o.redo();
                }
            }
            if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::Z))) {
                if let Some(o) = self.open.as_mut() {
                    o.undo();
                }
            }
        }
    }

    fn drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if let Some(p) = dropped.into_iter().next() {
            self.guard(After::Open(p));
        }
    }

    fn hover_overlay(&self, ctx: &egui::Context, p: &Pal) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop")));
        painter.rect_filled(rect, CornerRadius::ZERO, p.content.gamma_multiply(0.85));
        let r = rect.shrink(24.0);
        painter.rect_stroke(r, CornerRadius::same(16), Stroke::new(2.0, p.accent), egui::StrokeKind::Inside);
        painter.text(r.center() - vec2(0.0, 18.0), egui::Align2::CENTER_CENTER, I::FILE_ARROW_DOWN, theme::icon_font(44.0), p.accent);
        painter.text(r.center() + vec2(0.0, 26.0), egui::Align2::CENTER_CENTER, "Drop a job.toml, impose.toml or hot folder to open it", theme::bold(15.0), p.text);
    }

    // ------------------------------------------------------------ sidebar

    fn sidebar(&mut self, ui: &mut Ui, p: &Pal) {
        // Leave room for the traffic-light buttons (title bar is hidden on macOS).
        let top = if cfg!(target_os = "macos") && std::env::var_os("IMPOSE_SETTINGS_CLASSIC_TITLEBAR").is_none() { 44.0 } else { 14.0 };
        let (drag_rect, drag) = ui.allocate_exact_size(vec2(ui.available_width(), top), Sense::click_and_drag());
        window_drag(ui, &drag, drag_rect);

        // search
        egui::Frame::new()
            .fill(if p.dark { Color32::from_rgb(0x36, 0x36, 0x38) } else { Color32::from_rgb(0xDC, 0xDC, 0xE0) })
            .corner_radius(CornerRadius::same(7))
            .inner_margin(Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::ic(I::MAGNIFYING_GLASS).color(p.secondary));
                    let te = egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search")
                        .frame(egui::Frame::NONE)
                        .background_color(Color32::TRANSPARENT)
                        .desired_width(ui.available_width() - 22.0);
                    let r = ui.add(te);
                    if self.focus_search {
                        r.request_focus();
                        self.focus_search = false;
                    }
                    if !self.search.is_empty() && ui.add(egui::Button::new(theme::ic(I::X_CIRCLE).color(p.secondary)).frame(false)).clicked() {
                        self.search.clear();
                    }
                });
            });
        ui.add_space(10.0);

        let Some(o) = self.open.as_ref() else {
            ui.label(RichText::new("No file open").color(p.secondary));
            self.recent_list(ui, p);
            return;
        };

        // file card
        let kind = o.kind;
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            theme::tile(ui, if kind == FileKind::Job { egui_phosphor::fill::BOOK_OPEN } else { egui_phosphor::fill::COLUMNS }, if kind == FileKind::Job { [0x5E, 0x5C, 0xE6] } else { [0xFF, 0x2D, 0x55] }, 30.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(kind.title()).font(theme::bold(13.0)).color(p.text));
                ui.label(RichText::new(format!("{} · {}", kind.file_name(), kind.tool())).size(11.0).color(p.secondary));
            });
        })
        .response
        .on_hover_text(match kind {
            FileKind::Job => "sheet-impose job settings: sheet-fed and large-format imposition — work styles, folded signatures, marks and colour bars.",
            FileKind::Impose => "pdf-impose hot-folder settings: one row, bleed to bleed, with the eye mark, die spot and white underlay.",
        });
        ui.add_space(10.0);

        let searching = !self.search.trim().is_empty();
        let sections = kind.sections();
        let info: Vec<(bool, bool)> = sections
            .iter()
            .map(|s| (s.fields().any(|f| doc::has(&o.doc, f.key)), o.check.key.is_some_and(|k| s.fields().any(|f| f.key == k))))
            .collect();
        for (i, s) in sections.iter().enumerate() {
            let hits = if searching { s.fields().filter(|f| matches(f, &self.search)).count() } else { 0 };
            if searching && hits == 0 {
                continue;
            }
            let sel = !searching && i == self.section;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
            if sel {
                ui.painter().rect_filled(rect, CornerRadius::same(7), p.accent);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(7), p.hover);
            }
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
            theme::tile(&mut child, s.icon, s.color, 20.0);
            child.add_space(2.0);
            child.label(RichText::new(s.title).color(if sel { Color32::WHITE } else { p.text }));
            if searching {
                child.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(hits.to_string()).size(11.0).color(p.secondary));
                });
            }
            let (any_set, err_here) = info[i];
            if err_here {
                child.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::ic(I::WARNING_CIRCLE).color(if sel { Color32::WHITE } else { p.red }));
                });
            }
            let resp = resp.on_hover_text(format!("{}{}", s.subtitle, if any_set { "" } else { "\nNothing changed here — every setting uses its default." }));
            if resp.clicked() {
                self.section = i;
                self.search.clear();
            }
        }
        self.recent_list(ui, p);
    }

    fn recent_list(&mut self, ui: &mut Ui, p: &Pal) {
        if self.recent.is_empty() {
            return;
        }
        ui.add_space(14.0);
        ui.label(RichText::new("Recent").size(11.0).font(theme::bold(11.0)).color(p.secondary));
        ui.add_space(2.0);
        let current = self.open.as_ref().and_then(|o| o.path.clone()).and_then(|p| p.canonicalize().ok());
        let mut go = None;
        for r in self.recent.clone() {
            if Some(&r) == current.as_ref() {
                continue;
            }
            let folder = r.parent().and_then(|d| d.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(6), p.hover);
            }
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
            child.label(theme::ic(I::FILE_TEXT).color(p.secondary));
            child.add(egui::Label::new(RichText::new(format!("{folder} / {}", file_name(&r))).size(12.0).color(p.text)).truncate());
            if resp.on_hover_text(r.display().to_string()).clicked() {
                go = Some(r);
            }
        }
        if let Some(r) = go {
            self.guard(After::Open(r));
        }
    }

    // ------------------------------------------------------------ toolbar

    fn toolbar(&mut self, ui: &mut Ui, p: &Pal, now: f64) {
        let full = ui.max_rect();
        let drag = ui.interact(full, ui.id().with("drag"), Sense::click_and_drag());
        window_drag(ui, &drag, full);
        ui.horizontal_centered(|ui| {
            match &self.open {
                Some(o) => {
                    ui.vertical(|ui| {
                        ui.add_space(9.0);
                        ui.spacing_mut().item_spacing.y = 1.0;
                        let name = o.path.as_ref().map(|p| file_name(p)).unwrap_or_else(|| format!("Untitled {}", o.kind.file_name()));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(name).font(theme::bold(15.0)).color(p.text));
                            if o.dirty() {
                                ui.label(RichText::new("Edited").size(12.0).color(p.secondary));
                            }
                        });
                        let sub = match &o.path {
                            Some(path) => path.parent().map(|d| d.display().to_string()).unwrap_or_default(),
                            None => "Not saved yet".into(),
                        };
                        ui.add(egui::Label::new(RichText::new(sub).size(11.0).color(p.secondary)).truncate());
                    });
                }
                None => {
                    ui.label(RichText::new("Impose Settings").font(theme::bold(15.0)).color(p.text));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let has = self.open.is_some();
                let dirty = self.open.as_ref().is_some_and(|o| o.dirty() || o.path.is_none());
                if theme::tool_button(ui, I::CODE, has)
                    .on_hover_text(if self.show_toml { "Hide the TOML text" } else { "Show the file as TOML text, exactly as it will be saved" })
                    .clicked()
                {
                    self.show_toml = !self.show_toml;
                }
                if let Some(path) = self.open.as_ref().and_then(|o| o.path.clone()) {
                    if theme::tool_button(ui, I::FOLDER_OPEN, true).on_hover_text(if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in folder" }).clicked() {
                        reveal(&path);
                    }
                }
                ui.add_space(8.0);
                let (can_redo, can_undo) = self.open.as_ref().map(|o| (!o.redo.is_empty(), !o.undo.is_empty())).unwrap_or((false, false));
                if theme::tool_button(ui, I::ARROW_CLOCKWISE, can_redo).on_hover_text(keys("Redo  ⇧⌘Z", "Redo  Ctrl+Shift+Z")).clicked() {
                    self.open.as_mut().unwrap().redo();
                }
                if theme::tool_button(ui, I::ARROW_COUNTER_CLOCKWISE, can_undo).on_hover_text(keys("Undo  ⌘Z", "Undo  Ctrl+Z")).clicked() {
                    self.open.as_mut().unwrap().undo();
                }
                ui.add_space(8.0);
                let has_path = self.open.as_ref().is_some_and(|o| o.path.is_some());
                if theme::tool_button(ui, I::ARROW_U_UP_LEFT, has_path && self.open.as_ref().is_some_and(|o| o.dirty()))
                    .on_hover_text("Revert to the saved file")
                    .clicked()
                {
                    self.modal = Some(Modal::Unsaved(After::Revert));
                }
                if theme::tool_button(ui, I::FLOPPY_DISK, dirty).on_hover_text(keys("Save  ⌘S   (Save As…  ⇧⌘S)", "Save  Ctrl+S   (Save As…  Ctrl+Shift+S)")).clicked() {
                    self.save(false, now, false);
                }
                if theme::tool_button(ui, I::FOLDER_SIMPLE, true).on_hover_text(keys("Open a job.toml, impose.toml or hot folder  ⌘O", "Open a job.toml, impose.toml or hot folder  Ctrl+O")).clicked() {
                    self.guard(After::OpenDialog);
                }
                let new = theme::tool_button(ui, I::FILE_PLUS, true).on_hover_text(keys("New settings file  ⌘N", "New settings file  Ctrl+N"));
                egui::Popup::menu(&new).show(|ui| {
                    ui.set_min_width(250.0);
                    if ui.button(theme::itext(I::BOOK_OPEN, "New Sheet Job (job.toml)", 13.0, p.text)).on_hover_text("Settings for sheet-impose: sheets, work styles, binding, marks, colour bar.").clicked() {
                        self.guard(After::New(FileKind::Job, None));
                    }
                    if ui.button(theme::itext(I::COLUMNS, "New Row Hot Folder (impose.toml)", 13.0, p.text)).on_hover_text("Settings for pdf-impose: one row, eye mark, die spot and white underlay.").clicked() {
                        self.guard(After::New(FileKind::Impose, None));
                    }
                });
            });
        });
    }

    // ------------------------------------------------------------ status bar

    fn status_bar(&mut self, ui: &mut Ui, p: &Pal, now: f64) {
        let r = ui.max_rect();
        ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, p.separator));
        let Some(o) = self.open.as_ref() else { return };
        let mut jump = None;
        ui.horizontal_centered(|ui| {
            if let Some((msg, t)) = &self.status {
                if now - t < 3.0 {
                    ui.label(theme::itext(I::CHECK, msg, 12.0, p.green));
                    return;
                }
            }
            if o.check.ok {
                ui.label(theme::ic(I::CHECK_CIRCLE).color(p.green));
                ui.label(RichText::new(format!("Valid — {} will accept this file", o.kind.tool())).size(12.0).color(p.secondary));
                for n in &o.check.notes {
                    ui.label(theme::itext(I::WARNING, n, 12.0, p.orange));
                }
            } else {
                ui.label(theme::ic(I::WARNING_CIRCLE).color(p.red));
                let resp = ui
                    .add(egui::Label::new(RichText::new(&o.check.message).size(12.0).color(p.red)).truncate().sense(Sense::click()))
                    .on_hover_text(format!("{}\n\n{} will refuse this file until it is fixed.{}", o.check.message, o.kind.tool(), if o.check.key.is_some() { " Click to show the setting." } else { "" }));
                if resp.clicked() {
                    jump = o.check.key;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (set, total) = o.set_count();
                ui.label(RichText::new(format!("{set} of {total} settings in this file")).size(11.0).color(p.secondary))
                    .on_hover_text("Settings not in the file use their defaults. A dot next to a setting means it is written in the file; the ↺ button removes it again.");
            });
        });
        if let Some(k) = jump {
            self.go_to(k, now);
        }
    }

    fn go_to(&mut self, key: &'static str, now: f64) {
        if let Some(o) = &self.open {
            if let Some(i) = o.kind.sections().iter().position(|s| s.fields().any(|f| f.key == key)) {
                self.section = i;
                self.search.clear();
                self.flash = Some((key, now));
            }
        }
    }

    // ------------------------------------------------------------ TOML view

    fn toml_view(&mut self, ui: &mut Ui, p: &Pal) {
        let Some(o) = self.open.as_ref() else { return };
        ui.horizontal(|ui| {
            ui.label(theme::itext(I::CODE, &o.path.as_ref().map(|p| file_name(p)).unwrap_or_else(|| o.kind.file_name().into()), 13.0, p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button(theme::itext(I::COPY, "Copy", 12.0, p.text)).on_hover_text("Copy the whole file").clicked() {
                    ui.ctx().copy_text(o.cur.clone());
                }
            });
        });
        ui.add_space(6.0);
        let job = highlight(&o.doc.to_string(), p);
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.add(egui::Label::new(job).selectable(true).extend());
        });
    }

    // ------------------------------------------------------------ welcome

    fn welcome(&mut self, ui: &mut Ui, p: &Pal) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() / 2.0 - 190.0).max(30.0));
            theme::tile(ui, egui_phosphor::fill::SLIDERS_HORIZONTAL, [0x0A, 0x84, 0xFF], 72.0);
            ui.add_space(14.0);
            ui.label(RichText::new("Impose Settings").font(theme::bold(26.0)).color(p.text));
            ui.label(RichText::new("Create, edit and check settings for sheet-impose and pdf-impose.").color(p.secondary));
            ui.add_space(22.0);
            let w = 300.0;
            let big = |ui: &mut Ui, icon: &str, rgb: [u8; 3], title: &str, sub: &str, tip: &str| -> bool {
                let (rect, resp) = ui.allocate_exact_size(vec2(w, 54.0), Sense::click());
                let fill = if resp.hovered() { p.group.gamma_multiply(if p.dark { 1.3 } else { 0.97 }) } else { p.group };
                ui.painter().rect(rect, CornerRadius::same(10), fill, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
                let mut c = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(12.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
                theme::tile(&mut c, icon, rgb, 30.0);
                c.add_space(4.0);
                c.vertical(|ui| {
                    ui.add_space(9.0);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(title).font(theme::bold(13.0)).color(p.text));
                    ui.label(RichText::new(sub).size(11.0).color(p.secondary));
                });
                resp.on_hover_text(tip).clicked()
            };
            if big(ui, egui_phosphor::fill::FOLDER_OPEN, [0x1E, 0x90, 0xFF], "Open…", "A job.toml, impose.toml or hot folder", keys("Open an existing settings file (⌘O). You can also drop a file or folder on this window.", "Open an existing settings file (Ctrl+O). You can also drop a file or folder on this window.")) {
                self.guard(After::OpenDialog);
            }
            ui.add_space(8.0);
            if big(ui, egui_phosphor::fill::BOOK_OPEN, [0x5E, 0x5C, 0xE6], "New Sheet Job", "job.toml for sheet-impose", "Sheet-fed and large-format imposition: work styles, signatures, creep, marks, colour bars.") {
                self.guard(After::New(FileKind::Job, None));
            }
            ui.add_space(8.0);
            if big(ui, egui_phosphor::fill::COLUMNS, [0xFF, 0x2D, 0x55], "New Row Hot Folder", "impose.toml for pdf-impose", "One row, bleed to bleed, with the eye mark, die spot and white underlay.") {
                self.guard(After::New(FileKind::Impose, None));
            }
        });
    }

    // ------------------------------------------------------------ content

    fn content(&mut self, ui: &mut Ui, p: &Pal, now: f64) {
        let searching = !self.search.trim().is_empty();
        let kind = self.open.as_ref().unwrap().kind;
        let sections = kind.sections();
        self.section = self.section.min(sections.len() - 1);
        let mut events = Vec::new();
        let flash = self.flash.take();
        let mut keep_flash = None;

        egui::ScrollArea::vertical().auto_shrink(false).id_salt(("content", self.section, searching)).show(ui, |ui| {
            let w = ui.available_width();
            let cw = (w - 56.0).clamp(300.0, 700.0);
            let side = ((w - cw) / 2.0).max(16.0);
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(cw);
                    let o = self.open.as_mut().unwrap();
                    if searching {
                        ui.label(RichText::new(format!("Results for “{}”", self.search.trim())).font(theme::bold(20.0)).color(p.text));
                        ui.add_space(8.0);
                        let mut any = false;
                        for s in sections {
                            for g in s.groups {
                                let hits: Vec<&Field> = g.fields.iter().filter(|f| matches(f, &self.search)).collect();
                                if hits.is_empty() {
                                    continue;
                                }
                                any = true;
                                group(ui, p, &format!("{} › {}", s.title, g.title), "", |ui| {
                                    for (i, f) in hits.iter().enumerate() {
                                        if i > 0 {
                                            row_separator(ui, p);
                                        }
                                        events.extend(row(ui, p, o, f, flash, now, &mut keep_flash));
                                    }
                                });
                            }
                        }
                        if !any {
                            ui.add_space(30.0);
                            ui.vertical_centered(|ui| {
                                ui.label(theme::ic(I::MAGNIFYING_GLASS).size(32.0).color(p.tertiary));
                                ui.label(RichText::new("No settings match").color(p.secondary));
                            });
                        }
                    } else {
                        let s: &Section = &sections[self.section];
                        ui.horizontal(|ui| {
                            theme::tile(ui, s.icon, s.color, 36.0);
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(RichText::new(s.title).font(theme::bold(20.0)).color(p.text));
                                ui.label(RichText::new(s.subtitle).size(12.0).color(p.secondary));
                            });
                        });
                        ui.add_space(14.0);
                        for g in s.groups {
                            group(ui, p, g.title, g.footer, |ui| {
                                for (i, f) in g.fields.iter().enumerate() {
                                    if i > 0 {
                                        row_separator(ui, p);
                                    }
                                    events.extend(row(ui, p, o, f, flash, now, &mut keep_flash));
                                }
                            });
                        }
                    }
                    ui.add_space(24.0);
                });
            });
        });
        self.flash = keep_flash;

        for e in events {
            match e {
                Event::Units { from, to } => {
                    if from != to {
                        self.modal = Some(Modal::ConvertUnits { from, to });
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ modals

    fn modals(&mut self, ctx: &egui::Context, p: &Pal, now: f64) {
        let Some(modal) = self.modal.take() else { return };
        let mut keep = true;
        let mut then: Option<Box<dyn FnOnce(&mut App)>> = None;
        let resp = egui::Modal::new(egui::Id::new("alert"))
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(Margin::same(20)).corner_radius(CornerRadius::same(12)))
            .show(ctx, |ui| {
                ui.set_width(300.0);
                let alert = |ui: &mut Ui, icon: &str, color: Color32, title: &str, msg: &str| {
                    ui.vertical_centered(|ui| {
                        ui.label(theme::ic(icon).size(40.0).color(color));
                        ui.add_space(4.0);
                        ui.label(RichText::new(title).font(theme::bold(13.0)).color(p.text));
                        if !msg.is_empty() {
                            ui.add_space(2.0);
                            ui.label(RichText::new(msg).size(11.5).color(p.secondary));
                        }
                    });
                    ui.add_space(14.0);
                };
                match &modal {
                    Modal::Unsaved(after) => {
                        let revert = matches!(after, After::Revert);
                        if revert {
                            alert(ui, I::ARROW_U_UP_LEFT, p.orange, "Revert to the saved file?", "Your changes since the last save will be lost.");
                        } else {
                            let name = self.open.as_ref().and_then(|o| o.path.as_ref()).map(|p| file_name(p)).unwrap_or_else(|| "this new file".into());
                            alert(ui, I::FLOPPY_DISK, p.accent, &format!("Save changes to “{name}”?"), "Your changes will be lost if you don’t save them.");
                        }
                        ui.vertical_centered_justified(|ui| {
                            if revert {
                                if theme::primary_button(ui, "Revert").clicked() {
                                    keep = false;
                                    then = Some(Box::new(|a| a.perform(After::Revert)));
                                }
                            } else {
                                if theme::primary_button(ui, "Save").clicked() {
                                    keep = false;
                                    then = Some(Box::new(move |a: &mut App| a.save(false, now, false)));
                                }
                                if theme::button(ui, "Don’t Save").clicked() {
                                    keep = false;
                                    then = Some(Box::new(|a| {
                                        if let Some(o) = a.open.as_mut() {
                                            o.saved = o.cur.clone();
                                        }
                                    }));
                                }
                            }
                            if theme::button(ui, "Cancel").clicked() {
                                keep = false;
                            }
                        });
                    }
                    Modal::Error { title, msg } => {
                        alert(ui, I::WARNING_CIRCLE, p.red, title, msg);
                        ui.vertical_centered_justified(|ui| {
                            if theme::primary_button(ui, "OK").clicked() {
                                keep = false;
                            }
                        });
                    }
                    Modal::AskKind { path, text } => {
                        alert(ui, I::QUESTION, p.accent, &format!("What kind of file is “{}”?", file_name(path)), "Choose which tool these settings are for.");
                        ui.vertical_centered_justified(|ui| {
                            if theme::primary_button(ui, theme::itext(I::BOOK_OPEN, "Sheet Job (sheet-impose)", 13.0, Color32::WHITE)).clicked() {
                                keep = false;
                                let (pth, txt) = (path.clone(), text.clone());
                                then = Some(Box::new(move |a| a.load(FileKind::Job, &pth, &txt)));
                            }
                            if theme::button(ui, theme::itext(I::COLUMNS, "Row Hot Folder (pdf-impose)", 13.0, p.text)).clicked() {
                                keep = false;
                                let (pth, txt) = (path.clone(), text.clone());
                                then = Some(Box::new(move |a| a.load(FileKind::Impose, &pth, &txt)));
                            }
                            if theme::button(ui, "Cancel").clicked() {
                                keep = false;
                            }
                        });
                    }
                    Modal::NoSettings { dir } => {
                        alert(ui, I::FOLDER_DASHED, p.accent, &format!("“{}” has no settings file", file_name(dir)), "Create one for this hot folder? It is saved into the folder when you choose Save.");
                        ui.vertical_centered_justified(|ui| {
                            let d = dir.clone();
                            if theme::primary_button(ui, theme::itext(I::BOOK_OPEN, "New job.toml", 13.0, Color32::WHITE)).clicked() {
                                keep = false;
                                let d = d.clone();
                                then = Some(Box::new(move |a| a.perform(After::New(FileKind::Job, Some(d.join("job.toml"))))));
                            }
                            if theme::button(ui, theme::itext(I::COLUMNS, "New impose.toml", 13.0, p.text)).clicked() {
                                keep = false;
                                then = Some(Box::new(move |a| a.perform(After::New(FileKind::Impose, Some(d.join("impose.toml"))))));
                            }
                            if theme::button(ui, "Cancel").clicked() {
                                keep = false;
                            }
                        });
                    }
                    Modal::ConvertUnits { from, to } => {
                        alert(
                            ui,
                            I::RULER,
                            p.accent,
                            &format!("Convert lengths to {}?", unit_name(to)),
                            &format!("Every length is changed from {} to {} so the layout stays the same size. Lengths still at their default are written into the file too.", unit_name(from), unit_name(to)),
                        );
                        ui.vertical_centered_justified(|ui| {
                            if theme::primary_button(ui, "Convert").clicked() {
                                keep = false;
                                let (f, t) = (from.clone(), to.clone());
                                then = Some(Box::new(move |a| {
                                    if let Some(o) = a.open.as_mut() {
                                        o.convert_units(&f, &t);
                                    }
                                }));
                            }
                            if theme::button(ui, "Keep the Numbers").on_hover_text("Only the unit changes: 19 in becomes 19 mm.").clicked() {
                                keep = false;
                            }
                        });
                    }
                    Modal::SaveInvalid { path, msg } => {
                        alert(ui, I::WARNING, p.orange, "This file has a problem", &format!("{msg}\n\nThe tool will refuse it until it is fixed."));
                        ui.vertical_centered_justified(|ui| {
                            if theme::button(ui, "Save Anyway").clicked() {
                                keep = false;
                                let path = path.clone();
                                then = Some(Box::new(move |a| {
                                    if let Some(o) = a.open.as_mut() {
                                        o.path = Some(path);
                                    }
                                    a.save(false, now, true);
                                }));
                            }
                            if theme::primary_button(ui, "Fix It").clicked() {
                                keep = false;
                                then = Some(Box::new(move |a| {
                                    if let Some(k) = a.open.as_ref().and_then(|o| o.check.key) {
                                        a.go_to(k, now);
                                    }
                                }));
                            }
                        });
                    }
                }
            });
        if resp.should_close() {
            keep = false;
        }
        // "Save" in the unsaved-changes alert continues with the pending action.
        let pending = match (&modal, &then, keep) {
            (Modal::Unsaved(_), Some(_), false) => true,
            _ => false,
        };
        if keep {
            self.modal = Some(modal);
        } else if let Some(f) = then {
            f(self);
            if pending {
                if let Modal::Unsaved(after) = modal {
                    if !matches!(after, After::Revert) {
                        let still_dirty = self.open.as_ref().is_some_and(|o| o.dirty());
                        if !still_dirty && self.modal.is_none() {
                            self.perform(after);
                        }
                    }
                }
            }
        }
    }

}

/// Shortcut text for this platform: ⌘ on macOS, Ctrl elsewhere.
fn keys(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

fn unit_name(u: &str) -> &'static str {
    match u {
        "mm" => "millimetres",
        "pt" => "points",
        _ => "inches",
    }
}

fn window_drag(ui: &Ui, resp: &egui::Response, _rect: egui::Rect) {
    if resp.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
    }
}

fn matches(f: &Field, q: &str) -> bool {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    // label and key match anywhere; the help text only at the start of a word
    let word_start = |text: &str, w: &str| {
        let t = text.to_lowercase();
        t.match_indices(w).any(|(i, _)| i == 0 || !t[..i].chars().next_back().is_some_and(char::is_alphanumeric))
    };
    q.split_whitespace().all(|w| f.label.to_lowercase().contains(w) || f.key.to_lowercase().contains(w) || word_start(f.help, w))
}

// ================================================================== groups & rows

fn group(ui: &mut Ui, p: &Pal, title: &str, footer: &str, body: impl FnOnce(&mut Ui)) {
    if !title.is_empty() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(title).font(theme::bold(13.0)).color(p.text));
        });
        ui.add_space(2.0);
    }
    theme::group_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        body(ui);
    });
    if !footer.is_empty() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(footer).size(11.0).color(p.secondary)).wrap());
        });
    }
    ui.add_space(16.0);
}

fn row_separator(ui: &mut Ui, p: &Pal) {
    let r = ui.available_rect_before_wrap();
    let y = r.top();
    ui.painter().line_segment([egui::pos2(r.left() + 30.0, y), egui::pos2(r.right(), y)], Stroke::new(1.0, p.separator));
}

fn unit_suffix(u: Unit, units: &str) -> String {
    match u {
        Unit::Len => format!(" {units}"),
        Unit::Pt => " pt".into(),
        Unit::Ms => " ms".into(),
        Unit::Pct => " %".into(),
        Unit::Plain => String::new(),
    }
}

fn speed(u: Unit, units: &str) -> (f64, usize) {
    match (u, units) {
        (Unit::Len, "mm") => (0.1, 3),
        (Unit::Len, "pt") => (0.25, 3),
        (Unit::Len, _) => (0.005, 4),
        (Unit::Pt, _) => (0.05, 3),
        (Unit::Pct, _) => (0.5, 2),
        _ => (1.0, 0),
    }
}

fn fmt_num(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() { "0".into() } else { s.to_string() }
}

/// Default value of a field as shown in tooltips.
fn default_text(f: &Field, units: &str) -> String {
    match f.ty {
        Ty::Num { def, unit, .. } => format!("{}{}", fmt_num(def), unit_suffix(unit, units)),
        Ty::OptNum { auto, .. } => format!("Automatic ({auto})"),
        Ty::Int { def, unit, .. } => format!("{def}{}", unit_suffix(unit, units)),
        Ty::IntChoice { def, opts } => opts.iter().find(|o| o.0 == def).map(|o| o.1.to_string()).unwrap_or(def.to_string()),
        Ty::Bool(b) => if b { "On".into() } else { "Off".into() },
        Ty::Choice { def, opts, .. } => opts.iter().find(|o| o.value == def).map(|o| o.label.to_string()).unwrap_or(def.into()),
        Ty::Text { def, .. } | Ty::PdfFile { def } => if def.is_empty() { "empty".into() } else { format!("“{def}”") },
        Ty::Cmyk(c) => format!("C {} M {} Y {} K {}", fmt_num(c[0]), fmt_num(c[1]), fmt_num(c[2]), fmt_num(c[3])),
        Ty::PageMap => "Automatic page order".into(),
    }
}

fn tooltip(ui: &mut Ui, p: &Pal, f: &Field, units: &str, disabled: Option<&str>, current: Option<&str>) {
    ui.set_max_width(330.0);
    ui.horizontal(|ui| {
        ui.label(theme::ic(f.icon).size(15.0).color(p.accent));
        ui.label(RichText::new(f.label).font(theme::bold(13.0)).color(p.text));
    });
    ui.add_space(2.0);
    ui.add(egui::Label::new(RichText::new(f.help).color(p.text)).wrap());
    if let Some(c) = current {
        ui.add_space(2.0);
        ui.add(egui::Label::new(RichText::new(c).size(12.0).color(p.secondary)).wrap());
    }
    if let Some(why) = disabled {
        ui.add_space(4.0);
        ui.label(theme::itext(I::INFO, why, 12.0, p.orange));
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new("Key").size(11.0).color(p.tertiary));
        ui.label(RichText::new(f.key).size(11.0).family(FontFamily::Monospace).color(p.secondary));
        ui.label(RichText::new("  Default").size(11.0).color(p.tertiary));
        ui.label(RichText::new(default_text(f, units)).size(11.0).color(p.secondary));
    });
}

/// One settings row: icon, label, control, reset button. Returns events for the app.
fn row(ui: &mut Ui, p: &Pal, o: &mut Open, f: &'static Field, flash: Option<(&'static str, f64)>, now: f64, keep_flash: &mut Option<(&'static str, f64)>) -> Vec<Event> {
    let mut events = Vec::new();
    let units = o.units();
    let enabled = f.when.map(|(rule, _)| rule(&o.eff())).unwrap_or(true);
    let why = if enabled { None } else { f.when.map(|w| w.1) };
    let set = doc::has(&o.doc, f.key);
    let is_err = !o.check.ok && o.check.key == Some(f.key);
    let current = current_note(o, f);

    let top = ui.cursor().top();
    let resp = ui.horizontal(|ui| {
        ui.set_min_height(38.0);
        let tip = |ui: &mut Ui| tooltip(ui, p, f, &units, why, current.as_deref());
        let ic = ui.add(egui::Label::new(theme::ic(f.icon).size(17.0).color(if enabled { p.accent } else { p.tertiary })).sense(Sense::hover()));
        ic.on_hover_ui(tip);
        ui.add_space(2.0);
        let lr = ui.add(egui::Label::new(RichText::new(f.label).color(if enabled { p.text } else { p.tertiary })).sense(Sense::hover()).truncate());
        let lrect = lr.rect;
        lr.on_hover_ui(tip);
        if set {
            ui.painter().circle_filled(egui::pos2(lrect.right() + 6.0, lrect.center().y), 2.5, p.accent);
        }
        if is_err {
            ui.label(theme::ic(I::WARNING_CIRCLE).color(p.red)).on_hover_text(&o.check.message);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // reset (keeps its space so controls line up)
            if set {
                let r = ui
                    .add(egui::Button::new(theme::ic(I::ARROW_COUNTER_CLOCKWISE).size(13.0).color(p.secondary)).frame(false).min_size(vec2(20.0, 20.0)))
                    .on_hover_text(format!("Remove from the file and use the default ({})", default_text(f, &units)));
                if r.clicked() {
                    doc::remove(&mut o.doc, f.key);
                    if matches!(f.ty, Ty::PageMap) {
                        o.map_buf = None;
                        o.map_err = None;
                    }
                }
            } else {
                ui.add_space(20.0 + ui.spacing().item_spacing.x);
            }
            ui.add_enabled_ui(enabled, |ui| {
                let r = control(ui, p, o, f, &units, &mut events);
                if let Some(r) = r {
                    r.on_hover_ui(tip).on_disabled_hover_ui(tip);
                }
            });
        });
    });
    let rect = resp.response.rect;

    // page map editor sits under its row
    if matches!(f.ty, Ty::PageMap) && doc::has(&o.doc, f.key) {
        page_map_editor(ui, p, o);
    }

    let full = egui::Rect::from_min_max(egui::pos2(rect.left() - 12.0, top), egui::pos2(rect.right() + 10.0, ui.cursor().top()));
    if let Some((k, t)) = flash {
        if k == f.key {
            if now - t < 0.05 {
                ui.scroll_to_rect(full, Some(Align::Center));
            }
            let a = (1.0 - ((now - t) / 1.6) as f32).clamp(0.0, 1.0);
            if a > 0.0 {
                ui.painter().rect_filled(full, CornerRadius::same(6), p.accent.gamma_multiply(0.18 * a));
                *keep_flash = Some((k, t));
                ui.ctx().request_repaint();
            }
        }
    }
    if is_err {
        ui.painter().rect_stroke(full.shrink(1.0), CornerRadius::same(6), Stroke::new(1.0, p.red.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    }
    events
}

/// A sentence about the current choice, for tooltips.
fn current_note(o: &Open, f: &Field) -> Option<String> {
    match f.ty {
        Ty::Choice { def, opts, .. } => {
            let v = doc::get_str(&o.doc, f.key).unwrap_or(def.to_string());
            opts.iter().find(|x| x.value == v).map(|x| format!("Now: {} — {}", x.label, x.help))
        }
        _ => None,
    }
}

fn control(ui: &mut Ui, p: &Pal, o: &mut Open, f: &'static Field, units: &str, events: &mut Vec<Event>) -> Option<egui::Response> {
    let key = f.key;
    match f.ty {
        Ty::Num { def, min, max, unit } => {
            let mut v = doc::get_f64(&o.doc, key).unwrap_or(def);
            let (s, dec) = speed(unit, units);
            let r = ui.add_sized(
                [110.0, 22.0],
                egui::DragValue::new(&mut v).speed(s).range(min..=max).max_decimals(dec).custom_formatter(|v, _| fmt_num(v)).suffix(unit_suffix(unit, units)),
            );
            if r.changed() {
                doc::set_value(&mut o.doc, key, doc::num(v));
            }
            Some(r)
        }
        Ty::OptNum { auto, sample, min, unit } => {
            let cur = doc::get_f64(&o.doc, key);
            let mut is_auto = cur.is_none();
            // right-to-left: the Auto checkbox is on the right, the value to its left
            let sw = ui.checkbox(&mut is_auto, "Auto").on_hover_text(format!("Automatic: {auto}. Untick to type your own value."));
            if sw.changed() {
                if is_auto {
                    doc::remove(&mut o.doc, key);
                } else {
                    doc::set_value(&mut o.doc, key, doc::num(sample));
                }
            }
            if is_auto {
                ui.label(RichText::new(auto).color(p.secondary));
                Some(sw)
            } else {
                let mut v = doc::get_f64(&o.doc, key).unwrap_or(sample);
                let (s, dec) = speed(unit, units);
                let r = ui.add_sized([110.0, 22.0], egui::DragValue::new(&mut v).speed(s).range(min..=10_000.0).max_decimals(dec).custom_formatter(|v, _| fmt_num(v)).suffix(unit_suffix(unit, units)));
                if r.changed() {
                    doc::set_value(&mut o.doc, key, doc::num(v));
                }
                Some(r)
            }
        }
        Ty::Int { def, min, max, unit } => {
            let mut v = doc::get_i64(&o.doc, key).unwrap_or(def);
            let s = if unit == Unit::Ms { 50.0 } else { 0.1 };
            let r = ui.add_sized([110.0, 22.0], egui::DragValue::new(&mut v).speed(s).range(min..=max).suffix(unit_suffix(unit, units)));
            if r.changed() {
                doc::set_value(&mut o.doc, key, toml_edit::Value::from(v));
            }
            Some(r)
        }
        Ty::IntChoice { def, opts } => {
            let v = doc::get_i64(&o.doc, key).unwrap_or(def);
            let text = opts.iter().find(|x| x.0 == v).map(|x| x.1.to_string()).unwrap_or_else(|| format!("{v} (not allowed)"));
            let mut pick = None;
            let r = egui::ComboBox::from_id_salt(key).selected_text(text).width(170.0).show_ui(ui, |ui| {
                for (n, label) in opts {
                    if ui.selectable_label(*n == v, *label).clicked() {
                        pick = Some(*n);
                    }
                }
            });
            if let Some(n) = pick {
                doc::set_value(&mut o.doc, key, toml_edit::Value::from(n));
            }
            Some(r.response)
        }
        Ty::Bool(def) => {
            let mut v = doc::get_bool(&o.doc, key).unwrap_or(def);
            let r = theme::switch(ui, &mut v);
            if r.changed() {
                doc::set_value(&mut o.doc, key, toml_edit::Value::from(v));
            }
            Some(r)
        }
        Ty::Choice { def, opts, seg } => {
            let v = doc::get_str(&o.doc, key).unwrap_or(def.to_string());
            let idx = opts.iter().position(|x| x.value == v);
            let mut pick = None;
            let r = if seg {
                let items: Vec<(&str, &str, &str)> = opts.iter().map(|x| ("", x.label, x.help)).collect();
                let resp = ui.scope(|ui| {
                    if let Some(i) = theme::segmented(ui, egui::Id::new(key), idx.unwrap_or(usize::MAX), &items) {
                        pick = Some(i);
                    }
                    if idx.is_none() {
                        ui.label(theme::itext(I::WARNING, &format!("“{v}”"), 13.0, p.red)).on_hover_text("This value isn’t one of the choices. Pick one to fix it.");
                    }
                });
                resp.response
            } else {
                let text = match idx {
                    Some(i) => theme::itext(opts[i].icon, opts[i].label, 13.0, p.text),
                    None => theme::itext(I::WARNING, &format!("“{v}”"), 13.0, p.red),
                };
                egui::ComboBox::from_id_salt(key)
                    .selected_text(text)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, x) in opts.iter().enumerate() {
                            if ui.selectable_label(Some(i) == idx, theme::itext(x.icon, x.label, 13.0, p.text)).on_hover_text(x.help).clicked() {
                                pick = Some(i);
                            }
                        }
                    })
                    .response
            };
            if let Some(i) = pick {
                let newv = opts[i].value;
                if key == "units" {
                    let from = v.clone();
                    doc::set_value(&mut o.doc, key, toml_edit::Value::from(newv));
                    events.push(Event::Units { from, to: newv.to_string() });
                } else {
                    doc::set_value(&mut o.doc, key, toml_edit::Value::from(newv));
                }
            }
            Some(r)
        }
        Ty::Text { def, tokens } => {
            let mut v = doc::get_str(&o.doc, key).unwrap_or(def.to_string());
            let mut changed = false;
            if !tokens.is_empty() {
                let b = ui.add(egui::Button::new(theme::ic(I::BRACKETS_CURLY).color(p.secondary)).frame(false)).on_hover_text("Insert a token");
                egui::Popup::menu(&b).show(|ui| {
                    for t in tokens {
                        if ui.button(RichText::new(*t).family(FontFamily::Monospace)).clicked() {
                            v.push_str(t);
                            changed = true;
                        }
                    }
                });
            }
            let r = ui.add(egui::TextEdit::singleline(&mut v).desired_width(if tokens.is_empty() { 160.0 } else { 240.0 }));
            if r.changed() || changed {
                doc::set_value(&mut o.doc, key, toml_edit::Value::from(v));
            }
            Some(r)
        }
        Ty::PdfFile { def } => {
            let mut v = doc::get_str(&o.doc, key).unwrap_or(def.to_string());
            let mut changed = false;
            if !v.is_empty() && ui.add(egui::Button::new(theme::ic(I::X_CIRCLE).color(p.secondary)).frame(false)).on_hover_text("Use the built-in bar").clicked() {
                v.clear();
                changed = true;
            }
            if ui.button("Choose…").on_hover_text("Pick the colour-bar PDF").clicked() {
                let base = o.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf);
                let mut d = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_title("Choose Colour Bar PDF");
                if let Some(b) = &base {
                    d = d.set_directory(b);
                }
                if let Some(pick) = d.pick_file() {
                    v = match base.as_ref().and_then(|b| pick.strip_prefix(b).ok()) {
                        Some(rel) => rel.to_string_lossy().replace('\\', "/"),
                        None => pick.to_string_lossy().to_string(),
                    };
                    changed = true;
                }
            }
            let r = ui.add(egui::TextEdit::singleline(&mut v).desired_width(200.0).hint_text("Built-in CMYK bar"));
            if r.changed() || changed {
                doc::set_value(&mut o.doc, key, toml_edit::Value::from(v));
            }
            Some(r)
        }
        Ty::Cmyk(def) => {
            let mut c = doc::get_cmyk(&o.doc, key).unwrap_or(def);
            let mut changed = false;
            let names = [("K", Color32::from_gray(40)), ("Y", Color32::from_rgb(0xE6, 0xC2, 0x00)), ("M", Color32::from_rgb(0xE6, 0x00, 0x7E)), ("C", Color32::from_rgb(0x00, 0x9F, 0xE3))];
            let mut last = None;
            for (i, (n, col)) in names.iter().enumerate() {
                let idx = 3 - i;
                let r = ui.add_sized([46.0, 22.0], egui::DragValue::new(&mut c[idx]).speed(0.5).range(0.0..=100.0).max_decimals(1).custom_formatter(|v, _| fmt_num(v)));
                changed |= r.changed();
                ui.label(RichText::new(*n).font(theme::bold(11.0)).color(if p.dark && *n == "K" { p.secondary } else { *col }));
                last = Some(r);
            }
            // swatch
            let (rect, sr) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
            let rgb = |x: f64| ((1.0 - x / 100.0) * (1.0 - c[3] / 100.0) * 255.0).round() as u8;
            ui.painter().rect(rect, CornerRadius::same(5), Color32::from_rgb(rgb(c[0]), rgb(c[1]), rgb(c[2])), Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
            sr.on_hover_text("Approximate screen colour");
            if changed {
                doc::set_value(&mut o.doc, key, doc::cmyk(c));
            }
            last
        }
        Ty::PageMap => {
            let mut custom = doc::has(&o.doc, key);
            let r = theme::switch(ui, &mut custom).on_hover_text("On = use your own page map below. Off = automatic page order.");
            if r.changed() {
                if custom {
                    let ex = "[ [ [4, 1], [2, 3] ] ]";
                    if let Ok(v) = ex.parse::<toml_edit::Value>() {
                        doc::set_value(&mut o.doc, key, v);
                    }
                } else {
                    doc::remove(&mut o.doc, key);
                }
                o.map_buf = None;
                o.map_err = None;
            }
            ui.label(RichText::new(if custom { "Custom" } else { "Automatic" }).color(p.secondary));
            Some(r)
        }
    }
}

fn page_map_editor(ui: &mut Ui, p: &Pal, o: &mut Open) {
    let key = "pages.map";
    if o.map_buf.is_none() {
        o.map_buf = doc::get_value(&o.doc, key).map(|v| {
            let mut v = v.clone();
            v.decor_mut().clear();
            v.to_string()
        });
    }
    let mut text = o.map_buf.clone().unwrap_or_default();
    ui.add_space(2.0);
    let r = ui.add(
        egui::TextEdit::multiline(&mut text)
            .font(FontId::monospace(13.0))
            .desired_rows(4)
            .desired_width(f32::INFINITY)
            .hint_text("[ [ [4, 1], [2, 3] ] ]"),
    );
    r.on_hover_text("map = [ signature, … ]\nsignature = [ front, back ] — or [ [front, back], [front, back] ] for several sheets\nfront / back = [ page, page, … ] in reading order, top-left first\npage = 3, 0 for a blank, or \"3@180\" to turn it");
    if text != o.map_buf.clone().unwrap_or_default() {
        o.map_buf = Some(text.clone());
        let parsed = text.trim().parse::<toml_edit::Value>().map_err(|e| tidy_error(&e.to_string())).and_then(|v| {
            let tv: toml::Value = toml::from_str::<toml::Table>(&format!("v = {}", text.trim()))
                .map_err(|e| tidy_error(&e.to_string()))?
                .remove("v")
                .ok_or("empty")?;
            pdf_impose::sheet::layout::parse_map(&tv).map_err(|e| format!("{e:#}"))?;
            Ok(v)
        });
        match parsed {
            Ok(v) => {
                doc::set_value(&mut o.doc, key, v);
                o.map_err = None;
            }
            Err(e) => o.map_err = Some(e),
        }
    }
    ui.add_space(4.0);
    match &o.map_err {
        Some(e) => ui.label(theme::itext(I::WARNING_CIRCLE, &format!("{e} — not saved until fixed"), 12.0, p.red)),
        None => ui.label(theme::itext(I::CHECK_CIRCLE, "Map is valid", 12.0, p.green)),
    };
    ui.add_space(8.0);
}

/// Colour the TOML text: comments, [headers], keys, strings, numbers.
fn highlight(text: &str, p: &Pal) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let font = FontId::monospace(12.0);
    let fmt = |c: Color32| TextFormat { font_id: font.clone(), color: c, ..Default::default() };
    let (comment, header, keyc, string, number) = if p.dark {
        (Color32::from_rgb(0x7F, 0x8C, 0x8D), Color32::from_rgb(0xFF, 0x7A, 0xB2), Color32::from_rgb(0x67, 0xB7, 0xFF), Color32::from_rgb(0xFC, 0x6A, 0x5D), Color32::from_rgb(0xD0, 0xBF, 0x69))
    } else {
        (Color32::from_rgb(0x8A, 0x8A, 0x8E), Color32::from_rgb(0xAD, 0x3D, 0xA4), Color32::from_rgb(0x0F, 0x68, 0xA0), Color32::from_rgb(0xC4, 0x1A, 0x16), Color32::from_rgb(0x1C, 0x00, 0xCF))
    };
    let mut job = LayoutJob::default();
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with('#') {
            job.append(line, 0.0, fmt(comment));
            continue;
        }
        // split off a trailing comment that is not inside a string
        let mut in_str = false;
        let mut cut = line.len();
        for (i, ch) in line.char_indices() {
            match ch {
                '"' => in_str = !in_str,
                '#' if !in_str => {
                    cut = i;
                    break;
                }
                _ => {}
            }
        }
        let (code, rest) = line.split_at(cut);
        if t.starts_with('[') {
            job.append(code, 0.0, fmt(header));
        } else if let Some(eq) = code.find('=') {
            job.append(&code[..eq], 0.0, fmt(keyc));
            job.append("=", 0.0, fmt(p.secondary));
            let val = &code[eq + 1..];
            let c = if val.trim_start().starts_with('"') {
                string
            } else if val.trim().chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '[') {
                number
            } else {
                p.text
            };
            job.append(val, 0.0, fmt(c));
        } else {
            job.append(code, 0.0, fmt(p.text));
        }
        if !rest.is_empty() {
            job.append(rest, 0.0, fmt(comment));
        }
    }
    job
}
