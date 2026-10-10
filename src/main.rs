//! pdf-impose — hot-folder single-row step-and-repeat imposition.
//!
//! Usage:
//!   pdf-impose                                 hotfolders.toml if present, else ./PDFIn
//!   pdf-impose [watch] HOT_FOLDER              watch one folder
//!   pdf-impose [watch] LIST.toml               watch every folder in a watch list
//!   pdf-impose init [HOT_FOLDER | LIST.toml]   create folder + impose.toml, or a watch list
//!   pdf-impose impose IN.pdf OUT.pdf [-c CONFIG]   one-off imposition
//!
//! The watch list may also contain `type = "sheet"` folders; they are run
//! with the sheet-impose engine by this same process.

use pdf_impose::row::{config, impose};
use pdf_impose::watcher;

use anyhow::{bail, Result};
use config::{Config, CONFIG_FILE_NAME};
use std::path::{Path, PathBuf};
use watcher::LIST_FILE_NAME;

/// A `.toml` argument is a watch list; anything else is a hot folder.
fn is_list(arg: &str) -> bool {
    let p = Path::new(arg);
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml")) && !p.is_dir()
}

fn watch_target(arg: &str) -> Result<()> {
    if is_list(arg) {
        watcher::watch_list(Path::new(arg))
    } else {
        watcher::watch(Path::new(arg))
    }
}

fn usage() -> ! {
    eprintln!(
        "pdf-impose {}\n\n\
         USAGE:\n  \
           pdf-impose                                    watch {LIST_FILE_NAME} if present, else ./PDFIn\n  \
           pdf-impose [watch] HOT_FOLDER                 watch one folder\n  \
           pdf-impose [watch] LIST.toml                  watch every folder in a watch list\n  \
           pdf-impose init [HOT_FOLDER]                  create folder + default {CONFIG_FILE_NAME}\n  \
           pdf-impose init LIST.toml                     create a starter watch list\n  \
           pdf-impose impose IN.pdf OUT.pdf [-c CONFIG]  impose a single file\n\n\
         Log level: RUST_LOG=debug|info|warn (default info)",
        env!("CARGO_PKG_VERSION")
    );
    std::process::exit(2)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_target(false)
        .init();
    if let Err(e) = run() {
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["-h" | "--help" | "help", ..] => usage(),
        [] | ["watch"] => {
            if Path::new(LIST_FILE_NAME).exists() {
                watcher::watch_list(Path::new(LIST_FILE_NAME))
            } else {
                watcher::watch(Path::new("PDFIn"))
            }
        }
        ["watch", target] => watch_target(target),
        ["init"] => watcher::init_folder(Path::new("PDFIn")).map(|_| ()),
        ["init", target] if is_list(target) => watcher::init_list(Path::new(target)),
        ["init", dir] => watcher::init_folder(Path::new(dir)).map(|_| ()),
        ["impose", input, output, rest @ ..] => {
            let cfg = match rest {
                [] => Config::default(),
                ["-c" | "--config", path] => Config::load(&PathBuf::from(path))?,
                _ => usage(),
            };
            let r = impose::impose_file(Path::new(input), Path::new(output), &cfg)?;
            let u = cfg.pt(1.0);
            log::info!(
                "{} page(s), {} across, {} sheet(s), sheet {:.4} x {:.4} {:?}",
                r.pages, r.across, r.sheets, r.sheet_w_pt / u, r.sheet_h_pt / u, cfg.units
            );
            Ok(())
        }
        [target] if !target.starts_with('-') => watch_target(target),
        _ => {
            usage();
            #[allow(unreachable_code)]
            { bail!("bad arguments") }
        }
    }
}
