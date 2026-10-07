//! Hot-folder loop: waits for PDFs to land in the watched folder, waits until
//! they stop growing, imposes them, then moves them to Processed / Error.

use crate::config::{AfterAction, Config, CONFIG_FILE_NAME, DEFAULT_CONFIG_TOML};
use crate::impose::impose_file;
use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};
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
fn process(hot: &Path, pdf: &Path, cfg: &Config) -> Result<()> {
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
        "✔ {} → {} | {} page(s), {} across, {} sheet(s), row {:.4} (bleed to bleed), sheet {:.4} x {:.4} {:?}, slot {:.4} ({:.0} ms)",
        pdf.file_name().unwrap().to_string_lossy(),
        final_path.file_name().unwrap().to_string_lossy(),
        rep.pages, rep.across, rep.sheets, rep.row_w_pt / u,
        rep.sheet_w_pt / u, rep.sheet_h_pt / u, cfg.units, rep.slot_w_pt / u,
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

pub fn watch(hot: &Path) -> Result<()> {
    let hot = hot.canonicalize().unwrap_or_else(|_| hot.to_path_buf());
    let cfg_path = init_folder(&hot)?;
    log::info!("watching {}  (config: {})", hot.display(), cfg_path.display());

    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = tx.send(());
        }
    })?;
    watcher.watch(&hot, RecursiveMode::NonRecursive)?;

    let mut pending: HashMap<PathBuf, Pending> = HashMap::new();
    let mut kept: HashMap<PathBuf, (u64, Option<SystemTime>)> = HashMap::new();
    let mut last_cfg_error = String::new();
    let mut cfg = Config::load(&cfg_path).unwrap_or_default();

    loop {
        let stable = Duration::from_millis(cfg.folders.stable_ms);
        let poll = Duration::from_millis(cfg.folders.poll_ms.max(200));
        // Wake on an event, or after a short tick while files are settling.
        let wait = if pending.is_empty() { poll } else { Duration::from_millis(250) };
        match rx.recv_timeout(wait) {
            Ok(()) => while rx.try_recv().is_ok() {},
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => anyhow::bail!("file watcher stopped"),
        }

        // ---- scan
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
                if kept.get(&path) == Some(&(len, mtime)) {
                    continue; // already done, after_success = keep
                }
                seen.push(path.clone());
                match pending.get_mut(&path) {
                    Some(p) if p.len == len && p.mtime == mtime => {}
                    Some(p) => { p.len = len; p.mtime = mtime; p.since = now; }
                    None => {
                        log::info!("detected {}", path.file_name().unwrap().to_string_lossy());
                        pending.insert(path, Pending { len, mtime, since: now });
                    }
                }
            }
        }
        pending.retain(|p, _| seen.contains(p));

        // ---- process settled files
        let ready: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, p)| p.len > 0 && now.duration_since(p.since) >= stable)
            .map(|(k, _)| k.clone())
            .collect();
        if ready.is_empty() {
            continue;
        }

        // Re-read config for every batch so edits apply live.
        match Config::load(&cfg_path) {
            Ok(c) => { cfg = c; last_cfg_error.clear(); }
            Err(e) => {
                let msg = format!("{e:#}");
                if msg != last_cfg_error {
                    log::error!("config error — fix {} and the queued PDFs will run: {msg}", cfg_path.display());
                    last_cfg_error = msg;
                }
                continue;
            }
        }

        let mut ready = ready;
        ready.sort();
        for pdf in ready {
            let p = pending.remove(&pdf).unwrap();
            match process(&hot, &pdf, &cfg) {
                Ok(()) => match cfg.folders.after_success {
                    AfterAction::Move => { move_to(&pdf, &resolve(&hot, &cfg.folders.processed)).map(|_| ()).unwrap_or_else(|e| log::error!("move failed: {e:#}")); }
                    AfterAction::Delete => { std::fs::remove_file(&pdf).unwrap_or_else(|e| log::error!("delete failed: {e}")); }
                    AfterAction::Keep => { kept.insert(pdf.clone(), (p.len, p.mtime)); }
                },
                Err(e) => {
                    log::error!("✘ {}: {e:#}", pdf.file_name().unwrap().to_string_lossy());
                    let err_dir = resolve(&hot, &cfg.folders.error);
                    match move_to(&pdf, &err_dir) {
                        Ok(dst) => { let _ = std::fs::write(dst.with_extension("error.txt"), format!("{e:#}\n")); }
                        Err(me) => {
                            log::error!("could not move to error folder: {me:#}");
                            kept.insert(pdf.clone(), (p.len, p.mtime));
                        }
                    }
                }
            }
        }
    }
}
