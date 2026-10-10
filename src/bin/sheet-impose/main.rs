//! sheet-impose — sheet-fed and large-format imposition.
//!
//!   sheet-impose IN.pdf [OUT.pdf] [-c job.toml] [--set key=value]… [--map '[[[4,1],[2,3]]]'] [--plan]
//!   sheet-impose watch [FOLDER | hotfolders.toml]
//!   sheet-impose init [job.toml | FOLDER | hotfolders.toml]

use anyhow::{bail, Context, Result};
use pdf_impose::sheet::{self, config::{Job, DEFAULT_JOB_TOML}, layout};
use pdf_impose::watcher::{self, Kind, LIST_FILE_NAME};
use std::path::{Path, PathBuf};

fn usage() -> ! {
    eprintln!(
        "sheet-impose {}\n\n\
         USAGE:\n  \
           sheet-impose IN.pdf [OUT.pdf] [options]   impose a PDF for sheet-fed / large-format output\n  \
           sheet-impose [watch] FOLDER               hot folder (settings in FOLDER/job.toml)\n  \
           sheet-impose [watch] hotfolders.toml      watch every folder in a watch list\n  \
           sheet-impose                              same as `watch`: {LIST_FILE_NAME} if present, else ./SheetIn\n  \
           sheet-impose init job.toml                write a commented job file with every setting\n  \
           sheet-impose init FOLDER                  create a hot folder with its job.toml\n\n\
         OPTIONS:\n  \
           -c, --config job.toml   job settings (default: built-in defaults)\n  \
           --set key=value         override any setting, e.g. --set binding.style=saddle\n  \
                                   --set sheet.width=28 --set press.work_style=perfect\n  \
           --map '[[[4,1],[2,3]]]' reassign pages (same as pages.map in the job file)\n  \
           --plan                  show the layout and page map, don't write a PDF\n\n\
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

fn is_toml(p: &str) -> bool {
    Path::new(p).extension().is_some_and(|e| e.eq_ignore_ascii_case("toml")) && !Path::new(p).is_dir()
}

fn watch_target(target: Option<&str>) -> Result<()> {
    match target {
        Some(t) if is_toml(t) => watcher::watch_list_kind(Path::new(t), Kind::Sheet),
        Some(t) => watcher::watch_kind(Path::new(t), Kind::Sheet),
        None if Path::new(LIST_FILE_NAME).exists() => watcher::watch_list_kind(Path::new(LIST_FILE_NAME), Kind::Sheet),
        None => watcher::watch_kind(Path::new("SheetIn"), Kind::Sheet),
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => return watch_target(None),
        Some("-h" | "--help" | "help") => usage(),
        Some("watch") => return watch_target(args.get(1).map(String::as_str)),
        // `sheet-impose hotfolders.toml` or `sheet-impose SomeFolder` = watch
        Some(t) if args.len() == 1 && (is_toml(t) || Path::new(t).is_dir()) => return watch_target(Some(t)),
        Some("init") => {
            let target = args.get(1).map(String::as_str).unwrap_or("job.toml");
            if Path::new(target).file_name().is_some_and(|n| n == LIST_FILE_NAME) {
                return watcher::init_list(Path::new(target));
            }
            if is_toml(target) {
                let path = PathBuf::from(target);
                if path.exists() {
                    bail!("{} already exists", path.display());
                }
                std::fs::write(&path, DEFAULT_JOB_TOML)?;
                log::info!("wrote {}", path.display());
            } else {
                watcher::init_folder_kind(Path::new(target), Kind::Sheet)?;
            }
            return Ok(());
        }
        _ => {}
    }

    let mut positional = Vec::new();
    let mut config: Option<PathBuf> = None;
    let mut overrides = Vec::new();
    let mut plan_only = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-c" | "--config" => config = Some(it.next().context("-c needs a file")?.into()),
            "--set" => overrides.push(it.next().context("--set needs key=value")?),
            "--map" => overrides.push(format!("pages.map={}", it.next().context("--map needs a value")?)),
            "--plan" => plan_only = true,
            s if s.starts_with('-') => usage(),
            _ => positional.push(PathBuf::from(a)),
        }
    }
    let input = positional.first().cloned().unwrap_or_else(|| usage());
    let job = Job::load(config.as_deref(), &overrides)?;
    let job_dir = config.as_deref().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default();

    let started = std::time::Instant::now();
    let prepared = sheet::prepare(&input, &job)?;
    sheet::log_plan(&prepared, &job, &input, "");
    let map = layout::format_map(&prepared.plan);
    if plan_only {
        println!("# page map ({} sheet(s)) — paste into [pages] to edit\n{map}", prepared.plan.sheets.len());
        return Ok(());
    }
    log::debug!("page map:\n{map}");

    let stem = input.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let output = match positional.get(1) {
        Some(p) => p.clone(),
        None => input.with_file_name(sheet::output_name(&job, &stem, prepared.plan.sheets.len())),
    };
    sheet::write(prepared, &job, &job_dir, &stem, &output)?;
    log::info!("✔ wrote {} ({:.0} ms)", output.display(), started.elapsed().as_secs_f64() * 1000.0);
    Ok(())
}
