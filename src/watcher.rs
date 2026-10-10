//! Hot-folder loop shared by both programs. Each watched folder is either a
//! `row` folder (pdf-impose engine, settings in `impose.toml`) or a `sheet`
//! folder (sheet-impose engine, settings in `job.toml`). PDFs are processed
//! once their size has stopped changing, then moved to Processed / Error.

use crate::row::config::{AfterAction, Config as RowConfig, CONFIG_FILE_NAME as ROW_CONFIG, DEFAULT_CONFIG_TOML as ROW_DEFAULT};
use crate::sheet::config::{Job, DEFAULT_JOB_TOML};
use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

pub const LIST_FILE_NAME: &str = "hotfolders.toml";
pub const SHEET_CONFIG: &str = "job.toml";

/// Which engine a hot folder runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// pdf-impose: single row on a roll/web.
    Row,
    /// sheet-impose: sheet-fed / large format.
    Sheet,
}

impl Kind {
    pub fn config_name(self) -> &'static str {
        match self {
            Kind::Row => ROW_CONFIG,
            Kind::Sheet => SHEET_CONFIG,
        }
    }
    fn default_toml(self) -> &'static str {
        match self {
            Kind::Row => ROW_DEFAULT,
            Kind::Sheet => DEFAULT_JOB_TOML,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Kind::Row => "row",
            Kind::Sheet => "sheet",
        }
    }
}

struct Pending {
    len: u64,
    mtime: Option<SystemTime>,
    since: Instant,
}

pub fn resolve(base: &Path, p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}

/// Create the hot folder and a commented default config for `kind` if missing.
pub fn init_folder_kind(hot: &Path, kind: Kind) -> Result<PathBuf> {
    std::fs::create_dir_all(hot).with_context(|| format!("creating {}", hot.display()))?;
    let cfg_path = hot.join(kind.config_name());
    if !cfg_path.exists() {
        std::fs::write(&cfg_path, kind.default_toml())?;
        log::info!("wrote default config {}", cfg_path.display());
    }
    Ok(cfg_path)
}

/// Create a row (pdf-impose) hot folder.
pub fn init_folder(hot: &Path) -> Result<PathBuf> {
    init_folder_kind(hot, Kind::Row)
}

fn is_candidate(p: &Path) -> bool {
    let name = match p.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    !name.starts_with('.')
        && !name.starts_with('~')
        && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Pick a non-clashing path in `dir` for `name`.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    for i in 1.. {
        let c = dir.join(if ext.is_empty() { format!("{stem}_{i}") } else { format!("{stem}_{i}.{ext}") });
        if !c.exists() {
            return c;
        }
    }
    unreachable!()
}

fn move_to(src: &Path, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("file.pdf");
    let dst = unique_path(dir, name);
    if std::fs::rename(src, &dst).is_err() {
        // different volume: copy + remove
        std::fs::copy(src, &dst)?;
        std::fs::remove_file(src)?;
    }
    Ok(dst)
}

pub fn output_name(template: &str, stem: &str, across: usize, sheets: usize, pages: usize) -> String {
    let mut s = template
        .replace("{name}", stem)
        .replace("{across}", &across.to_string())
        .replace("{sheets}", &sheets.to_string())
        .replace("{pages}", &pages.to_string());
    if !s.to_ascii_lowercase().ends_with(".pdf") {
        s.push_str(".pdf");
    }
    s
}

// ---------------------------------------------------------------- engines --

/// Loaded settings of a hot folder.
enum Engine {
    Row(RowConfig),
    Sheet(Box<Job>),
}

/// Folder handling, common to both engines.
struct FolderOpts {
    output: String,
    processed: String,
    error: String,
    after: AfterAction,
    stable_ms: u64,
    poll_ms: u64,
}

impl Engine {
    fn load(kind: Kind, path: &Path) -> Result<Engine> {
        Ok(match kind {
            Kind::Row => Engine::Row(RowConfig::load(path)?),
            Kind::Sheet => Engine::Sheet(Box::new(Job::load(Some(path), &[])?)),
        })
    }
    fn default_for(kind: Kind) -> Engine {
        match kind {
            Kind::Row => Engine::Row(RowConfig::default()),
            Kind::Sheet => Engine::Sheet(Box::default()),
        }
    }
    fn opts(&self) -> FolderOpts {
        match self {
            Engine::Row(c) => FolderOpts {
                output: c.folders.output.clone(),
                processed: c.folders.processed.clone(),
                error: c.folders.error.clone(),
                after: c.folders.after_success,
                stable_ms: c.folders.stable_ms,
                poll_ms: c.folders.poll_ms,
            },
            Engine::Sheet(j) => FolderOpts {
                output: j.folders.output.clone(),
                processed: j.folders.processed.clone(),
                error: j.folders.error.clone(),
                after: j.folders.after_success,
                stable_ms: j.folders.stable_ms,
                poll_ms: j.folders.poll_ms,
            },
        }
    }
}

/// Run one job. Returns Err only for problems with the PDF itself.
fn process(spec: &FolderSpec, pdf: &Path, engine: &Engine) -> Result<()> {
    let hot = &spec.dir;
    let label = &spec.label;
    let opts = engine.opts();
    let out_dir = resolve(hot, &opts.output);
    std::fs::create_dir_all(&out_dir)?;
    let stem = pdf.file_stem().and_then(|s| s.to_str()).unwrap_or("output").to_string();
    let started = Instant::now();

    match engine {
        Engine::Row(cfg) => {
            // Impose to a temp name first; the final name needs the across count.
            let tmp_out = out_dir.join(format!(".{stem}.imposing.pdf"));
            let rep = match crate::row::impose::impose_file(pdf, &tmp_out, cfg) {
                Ok(r) => r,
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp_out);
                    return Err(e);
                }
            };
            let final_name = output_name(&cfg.output.file_name, &stem, rep.across, rep.sheets, rep.pages);
            let final_path = unique_path(&out_dir, &final_name);
            std::fs::rename(&tmp_out, &final_path)?;
            let u = cfg.pt(1.0);
            log::info!(
                "[{label}] ✔ {} → {} | {} page(s), {} across, {} sheet(s), row {:.4} (bleed to bleed), sheet {:.4} x {:.4} {:?}, slot {:.4} ({:.0} ms)",
                pdf.file_name().unwrap().to_string_lossy(),
                final_path.file_name().unwrap().to_string_lossy(),
                rep.pages, rep.across, rep.sheets, rep.row_w_pt / u,
                rep.sheet_w_pt / u, rep.sheet_h_pt / u, cfg.units, rep.slot_w_pt / u,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
        Engine::Sheet(job) => {
            let prefix = format!("[{label}] ");
            let prepared = crate::sheet::prepare(pdf, job)?;
            crate::sheet::log_plan(&prepared, job, pdf, &prefix);
            let sheets = prepared.plan.sheets.len();
            let final_path = unique_path(&out_dir, &crate::sheet::output_name(job, &stem, sheets));
            let job_dir = spec.config.parent().map(Path::to_path_buf).unwrap_or_else(|| hot.clone());
            crate::sheet::write(prepared, job, &job_dir, &stem, &final_path)?;
            log::info!(
                "{prefix}✔ {} → {} | {} sheet(s), {} / {} ({:.0} ms)",
                pdf.file_name().unwrap().to_string_lossy(),
                final_path.file_name().unwrap().to_string_lossy(),
                sheets,
                job.press.work_style.name(),
                job.binding.style.name(),
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    Ok(())
}

// ------------------------------------------------------------ watch list --

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchList {
    #[serde(default, rename = "hotfolder")]
    hotfolders: Vec<HotFolderEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct HotFolderEntry {
    /// Folder to watch. Relative paths are relative to the watch-list file.
    path: String,
    /// row (pdf-impose) or sheet (sheet-impose). Default: guessed from the settings file.
    #[serde(default, rename = "type")]
    kind: Option<Kind>,
    /// Settings file. Default: <path>/impose.toml (row) or <path>/job.toml (sheet).
    #[serde(default)]
    config: Option<String>,
    /// Label used in the log. Default: the folder name.
    #[serde(default)]
    name: Option<String>,
    #[serde(default = "yes")]
    enabled: bool,
}

fn yes() -> bool {
    true
}

/// A resolved hot folder to watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSpec {
    pub label: String,
    pub dir: PathBuf,
    pub config: PathBuf,
    pub kind: Kind,
}

pub const DEFAULT_LIST_TOML: &str = r#"# ------------------------------------------------------------------
# Watch list: every [[hotfolder]] below is watched at once, by either program.
# Relative paths are relative to this file.
# Edits are picked up while running (folders added/removed live).
#
#   type = "row"    pdf-impose single-row imposition, settings in <path>/impose.toml
#   type = "sheet"  sheet-impose sheet/large-format imposition, settings in <path>/job.toml
#   (type can be left out: a folder with job.toml is "sheet", one with impose.toml is "row")
# ------------------------------------------------------------------

[[hotfolder]]
path = "PDFIn"
type = "row"
# config  = "PDFIn/impose.toml"   # optional, default <path>/impose.toml or <path>/job.toml (can be shared)
# name    = "PDFIn"               # optional label for the log
# enabled = true                  # false = skip this folder

# [[hotfolder]]
# path = "SheetIn/Booklets"
# type = "sheet"

# [[hotfolder]]
# path   = "D:/Jobs/Labels/In"        # absolute paths work too (Windows: use / or '...')
# config = "shared/labels.toml"       # share one settings file between folders
"#;

/// Decide the engine of a folder: explicit type > settings file name > files present > default.
fn detect_kind(explicit: Option<Kind>, config: Option<&Path>, dir: &Path, default: Kind) -> Kind {
    if let Some(k) = explicit {
        return k;
    }
    if let Some(name) = config.and_then(|c| c.file_name()).and_then(|n| n.to_str()) {
        if name.eq_ignore_ascii_case(SHEET_CONFIG) {
            return Kind::Sheet;
        }
        if name.eq_ignore_ascii_case(ROW_CONFIG) {
            return Kind::Row;
        }
        // any other name: look inside for a sheet section
        if let Ok(text) = std::fs::read_to_string(config.unwrap()) {
            if text.contains("[press]") || text.contains("[binding]") {
                return Kind::Sheet;
            }
            if text.contains("[mark]") || text.contains("[layout]") {
                return Kind::Row;
            }
        }
    }
    let has_sheet = dir.join(SHEET_CONFIG).exists();
    let has_row = dir.join(ROW_CONFIG).exists();
    match (has_sheet, has_row) {
        (true, false) => Kind::Sheet,
        (false, true) => Kind::Row,
        _ => default,
    }
}

pub fn load_list(list_path: &Path, default: Kind) -> Result<Vec<FolderSpec>> {
    let text = std::fs::read_to_string(list_path)
        .with_context(|| format!("reading {}", list_path.display()))?;
    let list: WatchList =
        toml::from_str(&text).with_context(|| format!("parsing {}", list_path.display()))?;
    let base = list_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let base = if base.as_os_str().is_empty() { PathBuf::from(".") } else { base };

    let mut out: Vec<FolderSpec> = Vec::new();
    for e in list.hotfolders.into_iter().filter(|e| e.enabled) {
        anyhow::ensure!(!e.path.trim().is_empty(), "a [[hotfolder]] has an empty path");
        let dir = resolve(&base, &e.path);
        let dir = dir.canonicalize().unwrap_or(dir);
        let explicit_cfg = e.config.as_ref().map(|c| resolve(&base, c));
        let kind = detect_kind(e.kind, explicit_cfg.as_deref(), &dir, default);
        let config = explicit_cfg.unwrap_or_else(|| dir.join(kind.config_name()));
        let label = e.name.clone().unwrap_or_else(|| {
            dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| e.path.clone())
        });
        if out.iter().any(|f| f.dir == dir) {
            log::warn!("{} is listed twice in {}; using the first entry", dir.display(), list_path.display());
            continue;
        }
        out.push(FolderSpec { label, dir, config, kind });
    }
    Ok(out)
}

/// Write a starter watch list if none exists.
pub fn init_list(list_path: &Path) -> Result<()> {
    if !list_path.exists() {
        if let Some(parent) = list_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(list_path, DEFAULT_LIST_TOML)?;
        log::info!("wrote watch list {}", list_path.display());
    }
    Ok(())
}

// ------------------------------------------------------------------ loop --

struct Folder {
    spec: FolderSpec,
    engine: Engine,
    pending: HashMap<PathBuf, Pending>,
    kept: HashMap<PathBuf, (u64, Option<SystemTime>)>,
    last_cfg_error: String,
}

impl Folder {
    fn open(spec: FolderSpec) -> Result<Folder> {
        std::fs::create_dir_all(&spec.dir).with_context(|| format!("creating {}", spec.dir.display()))?;
        if !spec.config.exists() {
            if let Some(parent) = spec.config.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&spec.config, spec.kind.default_toml())?;
            log::info!("[{}] wrote default config {}", spec.label, spec.config.display());
        }
        let engine = match Engine::load(spec.kind, &spec.config) {
            Ok(e) => e,
            Err(e) => {
                log::error!("[{}] {e:#}", spec.label);
                Engine::default_for(spec.kind)
            }
        };
        log::info!(
            "[{}] watching {}  ({} — {})",
            spec.label,
            spec.dir.display(),
            spec.kind.name(),
            spec.config.display()
        );
        Ok(Folder { spec, engine, pending: HashMap::new(), kept: HashMap::new(), last_cfg_error: String::new() })
    }

    /// Scan the folder and run any settled PDFs.
    fn tick(&mut self) {
        let hot = self.spec.dir.clone();
        let label = self.spec.label.clone();
        let stable = Duration::from_millis(self.engine.opts().stable_ms);
        let now = Instant::now();
        let mut seen = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&hot) {
            for entry in rd.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                if !meta.is_file() || !is_candidate(&path) {
                    continue;
                }
                let (len, mtime) = (meta.len(), meta.modified().ok());
                if self.kept.get(&path) == Some(&(len, mtime)) {
                    continue; // already done, after_success = keep
                }
                seen.push(path.clone());
                match self.pending.get_mut(&path) {
                    Some(p) if p.len == len && p.mtime == mtime => {}
                    Some(p) => { p.len = len; p.mtime = mtime; p.since = now; }
                    None => {
                        log::info!("[{label}] detected {}", path.file_name().unwrap().to_string_lossy());
                        self.pending.insert(path, Pending { len, mtime, since: now });
                    }
                }
            }
        }
        self.pending.retain(|p, _| seen.contains(p));

        let mut ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, p)| p.len > 0 && now.duration_since(p.since) >= stable)
            .map(|(k, _)| k.clone())
            .collect();
        if ready.is_empty() {
            return;
        }

        // Re-read settings for every batch so edits apply live.
        match Engine::load(self.spec.kind, &self.spec.config) {
            Ok(e) => { self.engine = e; self.last_cfg_error.clear(); }
            Err(e) => {
                let msg = format!("{e:#}");
                if msg != self.last_cfg_error {
                    log::error!("[{label}] config error — fix {} and the queued PDFs will run: {msg}", self.spec.config.display());
                    self.last_cfg_error = msg;
                }
                return;
            }
        }
        let opts = self.engine.opts();

        ready.sort();
        for pdf in ready {
            let p = self.pending.remove(&pdf).unwrap();
            match process(&self.spec, &pdf, &self.engine) {
                Ok(()) => match opts.after {
                    AfterAction::Move => { move_to(&pdf, &resolve(&hot, &opts.processed)).map(|_| ()).unwrap_or_else(|e| log::error!("[{label}] move failed: {e:#}")); }
                    AfterAction::Delete => { std::fs::remove_file(&pdf).unwrap_or_else(|e| log::error!("[{label}] delete failed: {e}")); }
                    AfterAction::Keep => { self.kept.insert(pdf.clone(), (p.len, p.mtime)); }
                },
                Err(e) => {
                    log::error!("[{label}] ✘ {}: {e:#}", pdf.file_name().unwrap().to_string_lossy());
                    let err_dir = resolve(&hot, &opts.error);
                    match move_to(&pdf, &err_dir) {
                        Ok(dst) => { let _ = std::fs::write(dst.with_extension("error.txt"), format!("{e:#}\n")); }
                        Err(me) => {
                            log::error!("[{label}] could not move to error folder: {me:#}");
                            self.kept.insert(pdf.clone(), (p.len, p.mtime));
                        }
                    }
                }
            }
        }
    }
}

/// Watch a single folder (no watch-list file). `default` is used when the
/// folder has neither impose.toml nor job.toml yet.
pub fn watch_kind(hot: &Path, default: Kind) -> Result<()> {
    let dir = hot.canonicalize().unwrap_or_else(|_| hot.to_path_buf());
    let label = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "hotfolder".into());
    let kind = detect_kind(None, None, &dir, default);
    let config = dir.join(kind.config_name());
    run(vec![FolderSpec { label, dir, config, kind }], None, default)
}

/// Watch a single row (pdf-impose) folder.
pub fn watch(hot: &Path) -> Result<()> {
    watch_kind(hot, Kind::Row)
}

/// Watch every folder named in a watch-list file; the list is reloaded when it changes.
pub fn watch_list_kind(list_path: &Path, default: Kind) -> Result<()> {
    init_list(list_path)?;
    let list_path = list_path.canonicalize().unwrap_or_else(|_| list_path.to_path_buf());
    let specs = load_list(&list_path, default)?;
    log::info!("watch list {} — {} folder(s)", list_path.display(), specs.len());
    run(specs, Some(list_path), default)
}

pub fn watch_list(list_path: &Path) -> Result<()> {
    watch_list_kind(list_path, Kind::Row)
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn run(specs: Vec<FolderSpec>, list_path: Option<PathBuf>, default: Kind) -> Result<()> {
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = tx.send(());
        }
    })?;

    let mut folders: Vec<Folder> = Vec::new();
    let add = |spec: FolderSpec, folders: &mut Vec<Folder>, watcher: &mut notify::RecommendedWatcher| {
        match Folder::open(spec.clone()) {
            Ok(f) => {
                if let Err(e) = watcher.watch(&f.spec.dir, RecursiveMode::NonRecursive) {
                    log::warn!("[{}] no live events ({e}); relying on rescans", f.spec.label);
                }
                folders.push(f);
            }
            Err(e) => log::error!("[{}] cannot use {}: {e:#}", spec.label, spec.dir.display()),
        }
    };
    for spec in specs {
        add(spec, &mut folders, &mut watcher);
    }
    if folders.is_empty() && list_path.is_none() {
        anyhow::bail!("no folder to watch");
    }

    // Watch the list file's directory so edits to it are noticed quickly.
    let mut list_mtime = list_path.as_deref().and_then(mtime);
    if let Some(lp) = &list_path {
        if let Some(parent) = lp.parent() {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }
    }
    let mut last_list_error = String::new();

    loop {
        let settling = folders.iter().any(|f| !f.pending.is_empty());
        let poll = folders.iter().map(|f| f.engine.opts().poll_ms).min().unwrap_or(3000).max(200);
        let wait = if settling { Duration::from_millis(250) } else { Duration::from_millis(poll) };
        match rx.recv_timeout(wait) {
            Ok(()) => while rx.try_recv().is_ok() {},
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => anyhow::bail!("file watcher stopped"),
        }

        // ---- reload the watch list if it changed
        if let Some(lp) = &list_path {
            let m = mtime(lp);
            if m != list_mtime {
                list_mtime = m;
                match load_list(lp, default) {
                    Ok(specs) => {
                        last_list_error.clear();
                        folders.retain(|f| {
                            let keep = specs.contains(&f.spec);
                            if !keep {
                                let _ = watcher.unwatch(&f.spec.dir);
                                log::info!("[{}] stopped watching {}", f.spec.label, f.spec.dir.display());
                            }
                            keep
                        });
                        for spec in specs {
                            if !folders.iter().any(|f| f.spec == spec) {
                                add(spec, &mut folders, &mut watcher);
                            }
                        }
                        log::info!("watch list reloaded — {} folder(s)", folders.len());
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        if msg != last_list_error {
                            log::error!("watch list error (keeping current folders): {msg}");
                            last_list_error = msg;
                        }
                    }
                }
            }
        }

        for f in folders.iter_mut() {
            f.tick();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_detection() {
        let d = std::env::temp_dir().join(format!("hf-kind-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(detect_kind(None, None, &d, Kind::Row), Kind::Row);
        assert_eq!(detect_kind(None, None, &d, Kind::Sheet), Kind::Sheet);
        std::fs::write(d.join(SHEET_CONFIG), "").unwrap();
        assert_eq!(detect_kind(None, None, &d, Kind::Row), Kind::Sheet);
        assert_eq!(detect_kind(Some(Kind::Row), None, &d, Kind::Sheet), Kind::Row);
        let shared = d.join("books.toml");
        std::fs::write(&shared, "[binding]\nstyle = \"saddle\"\n").unwrap();
        assert_eq!(detect_kind(None, Some(&shared), &d, Kind::Row), Kind::Sheet);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn list_with_types() {
        let d = std::env::temp_dir().join(format!("hf-list-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let lp = d.join(LIST_FILE_NAME);
        std::fs::write(&lp, "[[hotfolder]]\npath = \"A\"\n\n[[hotfolder]]\npath = \"B\"\ntype = \"sheet\"\n").unwrap();
        let v = load_list(&lp, Kind::Row).unwrap();
        assert_eq!(v[0].kind, Kind::Row);
        assert!(v[0].config.ends_with("impose.toml"));
        assert_eq!(v[1].kind, Kind::Sheet);
        assert!(v[1].config.ends_with("job.toml"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
