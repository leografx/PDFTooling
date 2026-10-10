//! Hot-folder loop: waits for PDFs to land in the watched folder, waits until
//! they stop growing, imposes them, then moves them to Processed / Error.

use crate::config::{AfterAction, Config, CONFIG_FILE_NAME, DEFAULT_CONFIG_TOML};
use crate::impose::impose_file;
use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

struct Pending {
    len: u64,
    mtime: Option<SystemTime>,
    since: Instant,
}

pub fn resolve(base: &Path, p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}

/// Create the hot folder and a commented default config if missing.
pub fn init_folder(hot: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(hot).with_context(|| format!("creating {}", hot.display()))?;
    let cfg_path = hot.join(CONFIG_FILE_NAME);
    if !cfg_path.exists() {
        std::fs::write(&cfg_path, DEFAULT_CONFIG_TOML)?;
        log::info!("wrote default config {}", cfg_path.display());
    }
    Ok(cfg_path)
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
fn unique_path(dir: &Path, name: &str) -> PathBuf {
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

/// Run one job. Returns Err only for problems with the PDF itself.
fn process(hot: &Path, pdf: &Path, cfg: &Config, label: &str) -> Result<()> {
    let out_dir = resolve(hot, &cfg.folders.output);
    std::fs::create_dir_all(&out_dir)?;
    let stem = pdf.file_stem().and_then(|s| s.to_str()).unwrap_or("output").to_string();

    let started = Instant::now();
    // Impose to a temp name first; final name needs the across count.
    let tmp_out = out_dir.join(format!(".{stem}.imposing.pdf"));
    let rep = impose_file(pdf, &tmp_out, cfg);
    let rep = match rep {
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
    Ok(())
}

// ------------------------------------------------------------ watch list --

pub const LIST_FILE_NAME: &str = "hotfolders.toml";

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
    /// Settings file. Default: <path>/impose.toml. Several folders may share one.
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
}

pub const DEFAULT_LIST_TOML: &str = r#"# ------------------------------------------------------------------
# pdf-impose watch list: every [[hotfolder]] below is watched at once.
# Relative paths are relative to this file.
# Edits are picked up while pdf-impose is running (folders added/removed live).
# ------------------------------------------------------------------

[[hotfolder]]
path = "PDFIn"
# config  = "PDFIn/impose.toml"   # optional, default <path>/impose.toml (can be shared)
# name    = "PDFIn"               # optional label for the log
# enabled = true                  # false = skip this folder

# [[hotfolder]]
# path   = "6RAutoImpose"

# [[hotfolder]]
# path   = "D:/Jobs/Labels/In"        # absolute paths work too (Windows: use / or '...')
# config = "shared/labels.toml"       # share one settings file between folders
"#;

pub fn load_list(list_path: &Path) -> Result<Vec<FolderSpec>> {
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
        let config = match &e.config {
            Some(c) => resolve(&base, c),
            None => dir.join(CONFIG_FILE_NAME),
        };
        let label = e.name.clone().unwrap_or_else(|| {
            dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| e.path.clone())
        });
        if out.iter().any(|f| f.dir == dir) {
            log::warn!("{} is listed twice in {}; using the first entry", dir.display(), list_path.display());
            continue;
        }
        out.push(FolderSpec { label, dir, config });
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
    cfg: Config,
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
            std::fs::write(&spec.config, DEFAULT_CONFIG_TOML)?;
            log::info!("[{}] wrote default config {}", spec.label, spec.config.display());
        }
        let cfg = match Config::load(&spec.config) {
            Ok(c) => c,
            Err(e) => {
                log::error!("[{}] {e:#}", spec.label);
                Config::default()
            }
        };
        log::info!("[{}] watching {}  (config: {})", spec.label, spec.dir.display(), spec.config.display());
        Ok(Folder { spec, cfg, pending: HashMap::new(), kept: HashMap::new(), last_cfg_error: String::new() })
    }

    /// Scan the folder and run any settled PDFs. Returns true if files are still settling.
    fn tick(&mut self) -> bool {
        let hot = self.spec.dir.clone();
        let label = self.spec.label.clone();
        let stable = Duration::from_millis(self.cfg.folders.stable_ms);
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
            return !self.pending.is_empty();
        }

        // Re-read settings for every batch so edits apply live.
        match Config::load(&self.spec.config) {
            Ok(c) => { self.cfg = c; self.last_cfg_error.clear(); }
            Err(e) => {
                let msg = format!("{e:#}");
                if msg != self.last_cfg_error {
                    log::error!("[{label}] config error — fix {} and the queued PDFs will run: {msg}", self.spec.config.display());
                    self.last_cfg_error = msg;
                }
                return true;
            }
        }
        let cfg = &self.cfg;

        ready.sort();
        for pdf in ready {
            let p = self.pending.remove(&pdf).unwrap();
            match process(&hot, &pdf, cfg, &label) {
                Ok(()) => match cfg.folders.after_success {
                    AfterAction::Move => { move_to(&pdf, &resolve(&hot, &cfg.folders.processed)).map(|_| ()).unwrap_or_else(|e| log::error!("[{label}] move failed: {e:#}")); }
                    AfterAction::Delete => { std::fs::remove_file(&pdf).unwrap_or_else(|e| log::error!("[{label}] delete failed: {e}")); }
                    AfterAction::Keep => { self.kept.insert(pdf.clone(), (p.len, p.mtime)); }
                },
                Err(e) => {
                    log::error!("[{label}] ✘ {}: {e:#}", pdf.file_name().unwrap().to_string_lossy());
                    let err_dir = resolve(&hot, &cfg.folders.error);
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
        !self.pending.is_empty()
    }
}

/// Watch a single folder (no watch-list file).
pub fn watch(hot: &Path) -> Result<()> {
    let dir = hot.canonicalize().unwrap_or_else(|_| hot.to_path_buf());
    let label = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "hotfolder".into());
    let config = dir.join(CONFIG_FILE_NAME);
    run(vec![FolderSpec { label, dir, config }], None)
}

/// Watch every folder named in a watch-list file; the list is reloaded when it changes.
pub fn watch_list(list_path: &Path) -> Result<()> {
    init_list(list_path)?;
    let list_path = list_path.canonicalize().unwrap_or_else(|_| list_path.to_path_buf());
    let specs = load_list(&list_path)?;
    log::info!("watch list {} — {} folder(s)", list_path.display(), specs.len());
    run(specs, Some(list_path))
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn run(specs: Vec<FolderSpec>, list_path: Option<PathBuf>) -> Result<()> {
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
        let poll = folders.iter().map(|f| f.cfg.folders.poll_ms).min().unwrap_or(3000).max(200);
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
                match load_list(lp) {
                    Ok(specs) => {
                        last_list_error.clear();
                        // removed or changed entries
                        folders.retain(|f| {
                            let keep = specs.contains(&f.spec);
                            if !keep {
                                let _ = watcher.unwatch(&f.spec.dir);
                                log::info!("[{}] stopped watching {}", f.spec.label, f.spec.dir.display());
                            }
                            keep
                        });
                        // new entries
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
