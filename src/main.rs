//! pdf-impose — hot-folder single-row step-and-repeat imposition.
//!
//! Usage:
//!   pdf-impose [watch] [HOT_FOLDER]          watch a folder (default ./PDFIn)
//!   pdf-impose init [HOT_FOLDER]             create the folder + default impose.toml
//!   pdf-impose impose IN.pdf OUT.pdf [-c CONFIG]   one-off imposition

mod config;
mod impose;
mod watcher;

use anyhow::{bail, Result};
use config::{Config, CONFIG_FILE_NAME};
use std::path::{Path, PathBuf};

fn usage() -> ! {
    eprintln!(
        "pdf-impose {}\n\n\
         USAGE:\n  \
           pdf-impose [watch] [HOT_FOLDER]               watch a folder (default ./PDFIn)\n  \
           pdf-impose init [HOT_FOLDER]                  create folder + default {CONFIG_FILE_NAME}\n  \
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
        [] => watcher::watch(Path::new("PDFIn")),
        ["watch"] => watcher::watch(Path::new("PDFIn")),
        ["watch", dir] => watcher::watch(Path::new(dir)),
        ["init"] => watcher::init_folder(Path::new("PDFIn")).map(|_| ()),
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
        [dir] if !dir.starts_with('-') => watcher::watch(Path::new(dir)),
        _ => {
            usage();
            #[allow(unreachable_code)]
            { bail!("bad arguments") }
        }
    }
}
