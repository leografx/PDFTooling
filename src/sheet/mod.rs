//! The `sheet-impose` engine: sheet-fed / large-format imposition with work
//! styles, folded signatures, gang/cut & stack, marks and colour bars.

pub mod config;
pub mod fold;
pub mod layout;
pub mod render;

use crate::pdfcore::{self, PageGeom};
use anyhow::{Context, Result};
use config::Job;
use lopdf::Document;
use std::path::Path;

/// A loaded PDF with its imposition plan, ready to write.
pub struct Prepared {
    pub doc: Document,
    pub geoms: Vec<PageGeom>,
    pub plan: layout::Plan,
}

pub fn prepare(input: &Path, job: &Job) -> Result<Prepared> {
    let mut doc = Document::load(input).with_context(|| format!("opening {}", input.display()))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|e| anyhow::anyhow!("PDF is password protected: {e}"))?;
    }
    let geoms: Vec<PageGeom> = doc.get_pages().values().map(|&id| PageGeom::read(&doc, id)).collect();
    let plan = layout::plan(&layout::Inputs { job, pages: &geoms })?;
    Ok(Prepared { doc, geoms, plan })
}

/// Log the job summary and planner notes, each line prefixed (e.g. "[Books] ").
pub fn log_plan(p: &Prepared, job: &Job, input: &Path, prefix: &str) {
    let u = job.pt(1.0);
    let ul = job.units.label();
    let plan = &p.plan;
    log::info!(
        "{prefix}{}: {} page(s), trim {:.4} × {:.4} {ul}, bleed {:.4} {ul} | {} / {} | sheet {:.3} × {:.3} {ul}",
        input.file_name().unwrap_or_default().to_string_lossy(),
        p.geoms.len(),
        plan.page_w / u, plan.page_h / u, plan.bleed / u,
        job.press.work_style.name(), job.binding.style.name(),
        job.sheet.width,
        plan.sheets.first().map(|s| s.h / u).unwrap_or(job.sheet.height),
    );
    for n in &plan.notes {
        if n.starts_with("tip") || n.starts_with("warning") || n.contains("blank") || n.contains("differ") {
            log::warn!("{prefix}{n}");
        } else {
            log::info!("{prefix}{n}");
        }
    }
    let used: usize = plan.sheets.iter().map(|s| s.front.iter().chain(&s.back).filter(|x| x.page.is_some()).count()).sum();
    log::info!("{prefix}{} sheet(s), {} page positions printed", plan.sheets.len(), used);
}

/// Output file name from `output.file_name`.
pub fn output_name(job: &Job, stem: &str, sheets: usize) -> String {
    let mut s = job
        .output
        .file_name
        .replace("{name}", stem)
        .replace("{work}", job.press.work_style.name())
        .replace("{binding}", job.binding.style.name())
        .replace("{sheets}", &sheets.to_string());
    if !s.to_ascii_lowercase().ends_with(".pdf") {
        s.push_str(".pdf");
    }
    s
}

/// Render and save. `job_dir` resolves a relative colour-bar file.
pub fn write(p: Prepared, job: &Job, job_dir: &Path, stem: &str, output: &Path) -> Result<()> {
    let Prepared { mut doc, geoms, plan } = p;
    let r = render::render(&mut doc, job, &plan, &geoms, stem, job_dir)?;
    pdfcore::finish_and_save(&mut doc, r.pages_id, r.kids, output, job.output.object_streams)
}
