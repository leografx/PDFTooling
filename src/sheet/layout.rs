//! Turns a job + source pages into a plan: which page goes where, on which
//! side of which sheet, with what transform. Rendering is done separately.

use crate::sheet::config::{Align, Binding, BlankPages, CreepDirection, CreepMethod, Edge, Fill, Flip, Job, WorkStyle};
use crate::sheet::fold::{self, Side, Template};
use anyhow::{bail, Context, Result};
use crate::pdfcore::{Mat, PageGeom, Rect, EPS};

/// One page drawn on a plate.
#[derive(Debug, Clone)]
pub struct Placed {
    /// 0-based source page; None = blank position.
    pub page: Option<usize>,
    /// Page display coords (trim at origin) → plate coords.
    pub m: Mat,
    /// Trim rectangle on the plate.
    pub trim: Rect,
    /// Creep applied to this position (toward the spine; negative = outward).
    pub creep: f64,
    /// Unit vector from the spine toward the face, in plate coords ((0,0) = flat piece).
    pub face: (f64, f64),
}

#[derive(Debug, Clone)]
pub struct SheetPlan {
    pub w: f64,
    pub h: f64,
    /// Front plate (front view).
    pub front: Vec<Placed>,
    /// Back plate (back view, i.e. as the press prints it). Work & turn/tumble
    /// draws these on the same plate as `front`.
    pub back: Vec<Placed>,
    pub label: String,
    /// Signature numbers (1-based) on this sheet; empty for flat work.
    pub sigs: Vec<usize>,
}

#[derive(Debug)]
pub struct Plan {
    pub sheets: Vec<SheetPlan>,
    pub bleed: f64,
    pub page_w: f64,
    pub page_h: f64,
    pub flip: Flip,
    pub notes: Vec<String>,
}

/// Printable frame of a sheet: where blocks may go and where the colour bar sits.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// Area the trims of all blocks must stay inside (reserves for bleed + marks applied).
    pub area: Rect,
    pub bar: Option<Rect>,
}

/// Sheet margins (left, right, top, bottom) in points, gripper included.
pub fn margins(job: &Job) -> (f64, f64, f64, f64) {
    let s = &job.sheet;
    let (mut l, mut r, mut t, mut b) = (job.pt(s.margin_left), job.pt(s.margin_right), job.pt(s.margin_top), job.pt(s.margin_bottom));
    let g = job.pt(s.gripper);
    match s.gripper_edge {
        Edge::Left => l += g,
        Edge::Right => r += g,
        Edge::Top => t += g,
        Edge::Bottom => b += g,
    }
    (l, r, t, b)
}

/// Space reserved around every block edge: bleed plus room for marks.
pub fn mark_reserve(job: &Job, bleed: f64) -> f64 {
    let m = &job.marks;
    bleed + if m.trim || m.bleed { job.pt(m.offset + m.length) } else { 0.0 }
}

pub fn frame(job: &Job, w: f64, h: f64, bleed: f64) -> Frame {
    let (mut l, mut r, mut t, mut b) = margins(job);
    let duplex = job.press.work_style.duplex();
    // the back side must land on printable paper too
    if duplex {
        match job.flip() {
            Flip::Tumble => { let m = t.max(b); t = m; b = m; }
            _ => { let m = l.max(r); l = m; r = m; }
        }
    }
    let mut inner = Rect::new(l, b, w - r, h - t);
    let mut bar = None;
    let cb = &job.colorbar;
    if cb.enabled {
        let (th, gap) = (job.pt(cb.height), job.pt(cb.offset));
        let band = |e: Edge, inner: &Rect| match e {
            Edge::Top => Rect::new(inner.x0, inner.y1 - th, inner.x1, inner.y1),
            Edge::Bottom => Rect::new(inner.x0, inner.y0, inner.x1, inner.y0 + th),
            Edge::Left => Rect::new(inner.x0, inner.y0, inner.x0 + th, inner.y1),
            Edge::Right => Rect::new(inner.x1 - th, inner.y0, inner.x1, inner.y1),
        };
        bar = Some(band(cb.position, &inner));
        // keep the opposite band free too when the back flips onto it
        let opposite = |e: Edge| match e {
            Edge::Top => Edge::Bottom,
            Edge::Bottom => Edge::Top,
            Edge::Left => Edge::Right,
            Edge::Right => Edge::Left,
        };
        let mirrored = duplex
            && matches!(
                (job.flip(), cb.position),
                (Flip::Tumble, Edge::Top | Edge::Bottom) | (Flip::Turn | Flip::Auto, Edge::Left | Edge::Right)
            );
        let mut cut = vec![cb.position];
        if mirrored {
            cut.push(opposite(cb.position));
        }
        for e in cut {
            match e {
                Edge::Top => inner.y1 -= th + gap,
                Edge::Bottom => inner.y0 += th + gap,
                Edge::Left => inner.x0 += th + gap,
                Edge::Right => inner.x1 -= th + gap,
            }
        }
    }
    let res = mark_reserve(job, bleed);
    let mut area = inner.grow(-res);
    if area.w() < 0.0 { area.x1 = area.x0; }
    if area.h() < 0.0 { area.y1 = area.y0; }
    // work & turn / tumble: blocks on one half, their backs on the other
    match job.press.work_style {
        WorkStyle::Workturn => area.x1 = area.x1.min(w / 2.0 - res),
        WorkStyle::Worktumble => area.y1 = area.y1.min(h / 2.0 - res),
        _ => {}
    }
    Frame { area, bar }
}

// ------------------------------------------------------------ arranging --

/// Block placement: lower-left corner on the plate and whether it is turned 90°.
#[derive(Debug, Clone, Copy)]
pub struct Spot {
    pub x: f64,
    pub y: f64,
    pub rot: bool,
}

fn grid(aw: f64, ah: f64, w: f64, h: f64, gap: f64) -> (usize, usize) {
    if w > aw + EPS || h > ah + EPS || w <= 0.0 || h <= 0.0 {
        return (0, 0);
    }
    (((aw + gap + EPS) / (w + gap)).floor() as usize, ((ah + gap + EPS) / (h + gap)).floor() as usize)
}

fn fill_grid(out: &mut Vec<Spot>, x0: f64, y0: f64, nx: usize, ny: usize, w: f64, h: f64, gap: f64, rot: bool) {
    for j in 0..ny {
        for i in 0..nx {
            out.push(Spot { x: x0 + i as f64 * (w + gap), y: y0 + j as f64 * (h + gap), rot });
        }
    }
}

/// Fit as many `bw × bh` blocks as possible in `aw × ah` (origin 0,0).
/// Returns spots and a human-readable note about rotation.
pub fn arrange(aw: f64, ah: f64, bw: f64, bh: f64, gap: f64, allow_rot: bool, mixed: bool) -> (Vec<Spot>, Option<String>) {
    let (ux, uy) = grid(aw, ah, bw, bh, gap);
    let (rx, ry) = grid(aw, ah, bh, bw, gap);
    let up = ux * uy;
    let rotated = rx * ry;

    // best mixed layout: a block of one orientation plus a strip of the other
    let mut best_mixed: (usize, Vec<Spot>) = (0, Vec::new());
    for main_rot in [false, true] {
        let (mw, mh, ow, oh) = if main_rot { (bh, bw, bw, bh) } else { (bw, bh, bh, bw) };
        let (mx, my) = grid(aw, ah, mw, mh, gap);
        // columns of main + strip on the right
        for k in 1..mx {
            let sw = aw - k as f64 * (mw + gap);
            let (sx, sy) = grid(sw, ah, ow, oh, gap);
            let n = k * my + sx * sy;
            if n > best_mixed.0 && sx * sy > 0 {
                let mut v = Vec::new();
                fill_grid(&mut v, 0.0, 0.0, k, my, mw, mh, gap, main_rot);
                fill_grid(&mut v, k as f64 * (mw + gap), 0.0, sx, sy, ow, oh, gap, !main_rot);
                best_mixed = (n, v);
            }
        }
        // rows of main + strip on top
        for k in 1..my {
            let sh = ah - k as f64 * (mh + gap);
            let (sx, sy) = grid(aw, sh, ow, oh, gap);
            let n = k * mx + sx * sy;
            if n > best_mixed.0 && sx * sy > 0 {
                let mut v = Vec::new();
                fill_grid(&mut v, 0.0, 0.0, mx, k, mw, mh, gap, main_rot);
                fill_grid(&mut v, 0.0, k as f64 * (mh + gap), sx, sy, ow, oh, gap, !main_rot);
                best_mixed = (n, v);
            }
        }
    }

    let best_uniform = if allow_rot { up.max(rotated) } else { up };
    let mut note = None;
    let mut spots = Vec::new();
    if allow_rot && mixed && best_mixed.0 > best_uniform {
        let turned = best_mixed.1.iter().filter(|s| s.rot).count();
        note = Some(format!(
            "mixed rotation: {} upright + {} turned 90° = {} per sheet (best without mixing: {})",
            best_mixed.0 - turned, turned, best_mixed.0, best_uniform
        ));
        spots = best_mixed.1;
    } else if allow_rot && rotated > up {
        note = Some(format!("layout turned 90°: {rotated} per sheet instead of {up}"));
        fill_grid(&mut spots, 0.0, 0.0, rx, ry, bh, bw, gap, true);
    } else {
        fill_grid(&mut spots, 0.0, 0.0, ux, uy, bw, bh, gap, false);
        if best_mixed.0 > up.max(rotated) {
            note = Some(format!(
                "tip: turning some blocks 90° would fit {} instead of {} (set layout.mixed_rotation = true)",
                best_mixed.0, up
            ));
        } else if !allow_rot && rotated > up {
            note = Some(format!("tip: turning the layout 90° would fit {rotated} instead of {up} (layout.allow_rotation = true)"));
        }
    }
    (spots, note)
}

fn spot_size(s: &Spot, bw: f64, bh: f64) -> (f64, f64) {
    if s.rot { (bh, bw) } else { (bw, bh) }
}

/// Move spots so their bounding box is centred in (or pushed to the gripper of) `area`.
fn align_spots(spots: &mut [Spot], bw: f64, bh: f64, area: &Rect, job: &Job) {
    if spots.is_empty() {
        return;
    }
    let mut bb = Rect { x0: f64::MAX, y0: f64::MAX, x1: f64::MIN, y1: f64::MIN };
    for s in spots.iter() {
        let (w, h) = spot_size(s, bw, bh);
        bb = bb.union(&Rect::xywh(s.x, s.y, w, h));
    }
    let mut dx = area.x0 + (area.w() - bb.w()) / 2.0 - bb.x0;
    let mut dy = area.y0 + (area.h() - bb.h()) / 2.0 - bb.y0;
    if job.sheet.align == Align::Gripper {
        match job.sheet.gripper_edge {
            Edge::Bottom => dy = area.y0 - bb.y0,
            Edge::Top => dy = area.y1 - bb.y1,
            Edge::Left => dx = area.x0 - bb.x0,
            Edge::Right => dx = area.x1 - bb.x1,
        }
    }
    for s in spots.iter_mut() {
        s.x += dx;
        s.y += dy;
    }
}

/// Reading order: top row first, left to right.
fn reading_order(v: &mut [Placed]) {
    v.sort_by(|a, b| {
        let ya = (a.trim.cy() * 100.0).round() as i64;
        let yb = (b.trim.cy() * 100.0).round() as i64;
        yb.cmp(&ya).then(a.trim.cx().partial_cmp(&b.trim.cx()).unwrap())
    });
}

// ----------------------------------------------------------------- units --

/// One printable unit: a signature or a flat piece, with its global pages.
#[derive(Debug, Clone)]
struct Unit {
    tpl: usize,
    /// local page → global 0-based page (None = blank)
    pages: Vec<Option<usize>>,
    /// local page → creep amount (≥ 0)
    creep: Vec<f64>,
}

/// Content transform (page display coords) that applies creep: `amt` > 0
/// moves the face edge toward the spine by `amt`.
fn creep_matrix(method: CreepMethod, amt: f64, spine: (f64, f64), pw: f64, ph: f64) -> Mat {
    if amt == 0.0 || spine == (0.0, 0.0) {
        return Mat::I;
    }
    match method {
        CreepMethod::Shift => Mat::translate(spine.0 * amt, spine.1 * amt),
        CreepMethod::Scale => {
            // anchor on the middle of the spine edge, shrink so the face moves in by `amt`
            let (ax, ay, extent) = match spine {
                (x, _) if x < 0.0 => (0.0, ph / 2.0, pw),
                (x, _) if x > 0.0 => (pw, ph / 2.0, pw),
                (_, y) if y > 0.0 => (pw / 2.0, ph, ph),
                _ => (pw / 2.0, 0.0, ph),
            };
            let k = 1.0 - amt / extent;
            Mat::translate(-ax, -ay).then(&Mat::scale(k, k)).then(&Mat::translate(ax, ay))
        }
    }
}

/// Same as pdf-impose: put `k` units into `slots` positions.
fn distribute(k: usize, slots: usize, fill: Fill) -> Vec<Option<usize>> {
    if k == 0 || slots == 0 {
        return vec![None; slots];
    }
    if k >= slots {
        return (0..slots).map(Some).collect();
    }
    let base = slots / k;
    let extra = if fill == Fill::Repeat { slots % k } else { 0 };
    let base = if fill == Fill::Repeat { base } else { 1 };
    let mut out = Vec::with_capacity(slots);
    for i in 0..k {
        let c = base + usize::from(i < extra);
        out.extend(std::iter::repeat_n(Some(i), c));
    }
    out.resize(slots, None);
    out
}

/// Decompose `n` pages (multiple of 4) into signature sizes, largest first.
fn decompose(mut n: usize, max: usize, allowed: &[usize]) -> Vec<usize> {
    let mut v = Vec::new();
    while n > 0 {
        let s = allowed.iter().copied().filter(|&s| s <= max && s <= n).max().unwrap_or(4);
        v.push(s);
        n = n.saturating_sub(s);
    }
    v
}

/// Logical (padded) page index → real page, honouring where blanks go.
fn real_page(logical: usize, total: usize, real: usize, blanks: BlankPages) -> Option<usize> {
    if total == real {
        return Some(logical);
    }
    match blanks {
        BlankPages::End => (logical < real).then_some(logical),
        BlankPages::BeforeBackCover => {
            if logical == total - 1 {
                Some(real - 1)
            } else if logical < real - 1 {
                Some(logical)
            } else {
                None
            }
        }
    }
}

// ------------------------------------------------------------------ plan --

pub struct Inputs<'a> {
    pub job: &'a Job,
    pub pages: &'a [PageGeom],
}

pub fn plan(inp: &Inputs) -> Result<Plan> {
    let job = inp.job;
    let n_real = inp.pages.len();
    if n_real == 0 {
        bail!("the PDF has no pages");
    }
    let mut notes = Vec::new();
    let first = &inp.pages[0];

    // ---- product geometry
    let (dw, dh) = first.display_size();
    let pw = job.product.trim_width.map(|v| job.pt(v)).unwrap_or(dw);
    let ph = job.product.trim_height.map(|v| job.pt(v)).unwrap_or(dh);
    let bleed = match job.product.bleed {
        Some(b) => job.pt(b),
        None if first.has_bleed_box && first.file_bleed() > EPS => first.file_bleed(),
        None => 9.0, // 0.125 in
    };
    if first.file_bleed() + EPS < bleed {
        notes.push(format!(
            "the PDF has {:.4} {u} of bleed but {:.4} {u} is used — the extra is blank",
            first.file_bleed() / job.pt(1.0), bleed / job.pt(1.0), u = job.units.label()
        ));
    }
    let odd: Vec<usize> = inp
        .pages
        .iter()
        .enumerate()
        .filter(|(_, g)| {
            let (w, h) = g.display_size();
            (w - dw).abs() > 0.5 || (h - dh).abs() > 0.5
        })
        .map(|(i, _)| i + 1)
        .collect();
    if !odd.is_empty() {
        notes.push(format!("pages {odd:?} differ in size from page 1; they are centred in page-1-sized positions"));
    }

    let gutter = job.layout.gutter.map(|v| job.pt(v)).unwrap_or(2.0 * bleed);
    let fold_gap = job.layout.fold_gap.map(|v| job.pt(v)).unwrap_or(2.0 * bleed);
    let block_gap = job.layout.block_gap.map(|v| job.pt(v)).unwrap_or(2.0 * bleed);
    let duplex = job.press.work_style.duplex();
    let flip = job.flip();
    let binding = job.binding.style;

    let w = job.pt(job.sheet.width);
    let roll = job.sheet.height <= 0.0;
    let h_fixed = job.pt(job.sheet.height);

    // ---- templates + units
    let mut templates: Vec<Template> = Vec::new();
    let mut units: Vec<Unit> = Vec::new();
    let mut piece_gap = block_gap;

    if binding.folded() {
        let n_logical = n_real.div_ceil(4) * 4;
        if n_logical != n_real {
            notes.push(format!(
                "{} blank page(s) added to make {} pages ({})",
                n_logical - n_real,
                n_logical,
                match job.binding.blank_pages { BlankPages::End => "at the end", BlankPages::BeforeBackCover => "before the back cover" }
            ));
        }
        let spine_gap = if binding == Binding::Perfect { 2.0 * job.pt(job.binding.grind) } else { 0.0 };
        let lip = fold::Lip {
            side: match job.binding.lip_side {
                crate::sheet::config::Folio::None => fold::LipSide::None,
                crate::sheet::config::Folio::Low => fold::LipSide::Low,
                crate::sheet::config::Folio::High => fold::LipSide::High,
            },
            amount: job.pt(job.binding.lip),
        };
        if lip.side != fold::LipSide::None && lip.amount > 0.0 {
            notes.push(format!(
                "lip: {:.4} {} on the {} folio side of every signature",
                job.binding.lip,
                job.units.label(),
                if lip.side == fold::LipSide::Low { "low" } else { "high" }
            ));
        }
        let sizes: Vec<usize> = if job.binding.signature_pages > 0 { vec![job.binding.signature_pages, 4] } else { vec![32, 16, 8, 4] };
        // try each maximum signature size, keep the one using the fewest sheets
        let probe_h = if roll { f64::MAX / 4.0 } else { h_fixed };
        let fr = frame(job, w, if roll { 1e7 } else { probe_h }, bleed);
        // (repeats, sheets, max size, decomposition). With one signature per sheet,
        // a size whose signatures repeat on the sheet (≥ 2 copies) beats one that
        // only fits once; otherwise fewest sheets wins.
        let one_sig = job.layout.one_signature_per_sheet && !roll;
        let mut best: Option<(bool, usize, usize, Vec<usize>)> = None;
        let mut single_copy_sizes: Vec<usize> = Vec::new();
        let one_sig_folio = job.layout.one_signature_per_sheet && job.binding.signature_pages == 0;
        let cands: Vec<usize> = if job.binding.signature_pages > 0 {
            vec![job.binding.signature_pages]
        } else if one_sig_folio {
            // one folding sheet per signature: a single-fold folio, every page upright,
            // copies repeated head to foot
            vec![4]
        } else {
            vec![32, 16, 8, 4]
        };
        for &max in &cands {
            if max > n_logical && max != 4 && job.binding.signature_pages == 0 {
                continue;
            }
            let parts = decompose(n_logical, max, &sizes);
            let mut sheets = 0usize;
            let mut ok = true;
            let mut min_copies = usize::MAX;
            for (&size, count) in count_sizes(&parts).iter() {
                let t = fold::signature(size, pw, ph, job.binding.edge, spine_gap, fold_gap, lip);
                let mixed = job.layout.mixed_rotation && !job.layout.one_signature_per_sheet;
                let (spots, _) = arrange(fr.area.w(), fr.area.h(), t.w, t.h, block_gap, job.layout.allow_rotation, mixed);
                if spots.is_empty() {
                    ok = false;
                    break;
                }
                min_copies = min_copies.min(spots.len());
                sheets += if roll {
                    1
                } else if job.layout.one_signature_per_sheet {
                    *count
                } else {
                    count.div_ceil(spots.len())
                };
            }
            if !ok {
                continue;
            }
            let repeats = one_sig && min_copies >= 2;
            if one_sig && !repeats {
                single_copy_sizes.push(max);
            }
            let better = match &best {
                None => true,
                Some((r, sh, _, _)) => (repeats && !r) || (repeats == *r && sheets < *sh),
            };
            if better {
                best = Some((repeats, sheets, max, parts));
            }
        }
        if let Some((repeats, _, max, _)) = &best {
            if one_sig && *repeats && single_copy_sizes.iter().any(|s| s > max) {
                let bigger: Vec<String> = single_copy_sizes.iter().filter(|s| *s > max).map(|s| format!("{s}pp")).collect();
                notes.push(format!(
                    "one signature per sheet: {} only fit(s) once on this sheet, so {max}pp signatures are used so they repeat",
                    bigger.join("/")
                ));
            } else if one_sig && !*repeats {
                notes.push("tip: one signature per sheet is on, but no signature size fits twice on this sheet — a larger sheet or smaller margins would allow repeats".into());
            }
        }
        let (_, _, max, parts) = best.with_context(|| {
            format!(
                "no signature fits the sheet: the smallest (4pp) needs {:.3} × {:.3} {u} plus bleed and marks",
                2.0 * pw / job.pt(1.0), ph / job.pt(1.0), u = job.units.label()
            )
        })?;
        if one_sig_folio {
            notes.push(format!(
                "one signature per sheet: {} single-fold folios (4pp), every copy upright and head to foot",
                parts.len()
            ));
        } else if job.binding.signature_pages == 0 {
            notes.push(format!("auto signature size: up to {max}pp → {}", describe_parts(&parts)));
        } else if job.layout.one_signature_per_sheet && job.binding.signature_pages > 4 {
            notes.push(format!(
                "one signature per sheet with {}pp signatures: copies all face the same way, but inside each signature the folded rows are heads-together",
                job.binding.signature_pages
            ));
        }
        // templates per size
        let mut tpl_of = std::collections::BTreeMap::new();
        for &s in &parts {
            tpl_of.entry(s).or_insert_with(|| {
                templates.push(fold::signature(s, pw, ph, job.binding.edge, spine_gap, fold_gap, lip));
                templates.len() - 1
            });
        }
        // creep: innermost amount, from `creep` or from the paper caliper
        let caliper = job.pt(job.binding.paper_caliper);
        let creep_total = |folios: usize| -> f64 {
            if job.binding.creep > 0.0 {
                job.pt(job.binding.creep)
            } else {
                caliper * folios.saturating_sub(1) as f64
            }
        };
        // page mapping
        let (mut lo, mut hi) = (0usize, n_logical - 1);
        let mut creep_rows: Vec<(usize, f64)> = Vec::new(); // (depth, amount) for the log
        for &s in &parts {
            let mut pages = Vec::with_capacity(s);
            let mut creep = Vec::with_capacity(s);
            for j in 0..s {
                let logical = match binding {
                    Binding::Saddle => if j < s / 2 { lo + j } else { hi - (s - 1 - j) },
                    _ => lo + j,
                };
                pages.push(logical);
                // folio depth from the outside: whole book (saddle) or this signature (perfect)
                let (depth, dmax, total) = match binding {
                    Binding::Saddle => (logical.min(n_logical - 1 - logical) / 2, n_logical / 4 - 1, creep_total(n_logical / 4)),
                    _ => (j.min(s - 1 - j) / 2, s / 4 - 1, creep_total(s / 4)),
                };
                let amt = if dmax == 0 { 0.0 } else { total * depth as f64 / dmax as f64 };
                if amt > 0.0 && !creep_rows.iter().any(|r| r.0 == depth) {
                    creep_rows.push((depth, amt));
                }
                creep.push(amt);
            }
            match binding {
                Binding::Saddle => { lo += s / 2; hi = hi.saturating_sub(s / 2); }
                _ => lo += s,
            }
            units.push(Unit {
                tpl: tpl_of[&s],
                pages: pages.into_iter().map(|p| real_page(p, n_logical, n_real, job.binding.blank_pages)).collect(),
                creep,
            });
        }
        if !creep_rows.is_empty() {
            creep_rows.sort_by_key(|r| r.0);
            // sanity check: creep above ~0.25 in or 3% of the page width is unusual
            let max = creep_rows.iter().map(|r| r.1).fold(0.0, f64::max);
            if max > 18.0 || max > 0.03 * pw {
                let u = job.pt(1.0);
                let ul = job.units.label();
                notes.push(format!(
                    "warning: creep reaches {:.4} {ul} at the centre ({:.1}% of the page width). {} \
                     Check the value — text stock is usually 0.003-0.006 in thick.",
                    max / u,
                    100.0 * max / pw,
                    match job.binding.creep_method {
                        CreepMethod::Shift => format!("With shift, up to {:.4} {ul} of each centre page is cut off at the spine; creep_method = \"scale\" keeps it.", max / u),
                        CreepMethod::Scale => format!("With scale, centre pages are reduced to {:.1}%.", 100.0 * (1.0 - max / pw)),
                    },
                ));
            }
            let u = job.pt(1.0);
            let table: Vec<String> = creep_rows
                .iter()
                .map(|(d, a)| {
                    if binding == Binding::Saddle {
                        let (p, q) = (2 * d + 1, n_logical - 2 * d);
                        format!("{p}-{}/{}-{q} {:.4}", p + 1, q - 1, a / u)
                    } else {
                        format!("folio {} {:.4}", d + 1, a / u)
                    }
                })
                .collect();
            notes.push(format!(
                "creep — {} {}{} ({}): {}",
                match job.binding.creep_method { CreepMethod::Shift => "shift", CreepMethod::Scale => "scale" },
                match job.binding.creep_direction { CreepDirection::In => "toward the spine", CreepDirection::Out => "away from the spine" },
                if job.binding.creep > 0.0 { String::new() } else { format!(", from caliper {}", job.binding.paper_caliper) },
                job.units.label(),
                table.join(", ")
            ));
        }
    } else {
        // flat / cut & stack
        let piece_flip = match job.binding.edge {
            Edge::Top | Edge::Bottom => Flip::Tumble,
            _ => Flip::Turn,
        };
        templates.push(fold::flat(pw, ph, duplex, piece_flip));
        let ppu = if duplex { 2 } else { 1 };
        for chunk in (0..n_real).collect::<Vec<_>>().chunks(ppu) {
            let mut pages: Vec<Option<usize>> = chunk.iter().map(|&p| Some(p)).collect();
            pages.resize(ppu, None);
            units.push(Unit { tpl: 0, pages, creep: vec![0.0; ppu] });
        }
        if duplex && n_real % 2 == 1 {
            notes.push("odd page count: the last piece has a blank back".into());
        }
        piece_gap = gutter;
    }

    // ---- sheets, one template group at a time
    let mut sheets: Vec<SheetPlan> = Vec::new();
    for (ti, tpl) in templates.iter().enumerate() {
        let group: Vec<usize> = (0..units.len()).filter(|&u| units[u].tpl == ti).collect();
        if group.is_empty() {
            continue;
        }
        // layout of blocks on a sheet
        let (sheet_h, spots_per_sheet, spots) = if roll {
            roll_layout(job, w, bleed, tpl, piece_gap, group.len(), &mut notes)?
        } else {
            let fr = frame(job, w, h_fixed, bleed);
            // one signature per sheet: every copy the same way round (no mixed rotation)
            let mixed = job.layout.mixed_rotation && !(binding.folded() && job.layout.one_signature_per_sheet);
            let (mut spots, note) = arrange(fr.area.w(), fr.area.h(), tpl.w, tpl.h, piece_gap, job.layout.allow_rotation, mixed);
            if spots.is_empty() {
                bail!(
                    "{} ({:.3} × {:.3} {u}) does not fit the printable area {:.3} × {:.3} {u}",
                    tpl.label,
                    tpl.w / job.pt(1.0), tpl.h / job.pt(1.0),
                    fr.area.w() / job.pt(1.0), fr.area.h() / job.pt(1.0),
                    u = job.units.label()
                );
            }
            if let Some(n) = note {
                notes.push(format!("{}: {n}", tpl.label));
            }
            for s in spots.iter_mut() {
                s.x += fr.area.x0;
                s.y += fr.area.y0;
            }
            align_spots(&mut spots, tpl.w, tpl.h, &fr.area, job);
            (h_fixed, spots.len(), spots)
        };
        notes.push(format!("{}: {} per sheet", tpl.label, spots_per_sheet));
        // fill positions in reading order (top row first, left to right) so
        // cut stacks and repeats run the way the sheet is read
        let mut spots = spots;
        spots.sort_by(|a, b| {
            let top = |s: &Spot| ((s.y + spot_size(s, tpl.w, tpl.h).1) * 100.0).round() as i64;
            top(b).cmp(&top(a)).then(a.x.partial_cmp(&b.x).unwrap())
        });

        // assign units to sheets
        let n = spots_per_sheet;
        let u = group.len();
        let assignment: Vec<Vec<Option<usize>>> = if binding == Binding::Cutstack {
            let stacks = u.div_ceil(n);
            (0..stacks).map(|k| (0..n).map(|s| (s * stacks + k < u).then(|| group[s * stacks + k])).collect()).collect()
        } else if roll {
            group.chunks(n).map(|c| c.iter().map(|&x| Some(x)).collect()).collect()
        } else if binding.folded() && job.layout.one_signature_per_sheet {
            // 1 folding sheet per signature: fill each sheet with copies of one signature
            if u > 0 {
                notes.push(if n == 1 {
                    format!("{}: one signature per sheet (only 1 fits, so no repeats)", tpl.label)
                } else {
                    format!("{}: one signature per sheet, {} copies each", tpl.label, n)
                });
            }
            group.iter().map(|&x| vec![Some(x); n]).collect()
        } else if job.layout.gang {
            if u <= n {
                vec![distribute(u, n, job.layout.fill).into_iter().map(|o| o.map(|i| group[i])).collect()]
            } else {
                group
                    .chunks(n)
                    .map(|c| distribute(c.len(), n, job.layout.fill).into_iter().map(|o| o.map(|i| c[i])).collect())
                    .collect()
            }
        } else {
            group.iter().map(|&x| distribute(1, n, job.layout.fill).into_iter().map(|o| o.map(|_| x)).collect()).collect()
        };

        for slots in assignment {
            let mut counts = std::collections::HashMap::new();
            for u in slots.iter().flatten() {
                *counts.entry(*u).or_insert(0usize) += 1;
            }
            let copies = counts.values().copied().min().unwrap_or(1);
            if copies > 1 && binding != Binding::Cutstack {
                notes.push(format!(
                    "sheet {}: each {} appears {copies}× — print 1/{copies} of the quantity for this sheet",
                    sheets.len() + 1,
                    if binding.folded() { "signature" } else { "piece" }
                ));
            }
            let mut front = Vec::new();
            let mut back = Vec::new();
            let back_m = match flip {
                Flip::Tumble => Mat::mirror_y(sheet_h / 2.0),
                _ => Mat::mirror_x(w / 2.0),
            };
            for (spot, unit) in spots.iter().zip(slots.iter()) {
                let p = if spot.rot { Mat::rot90(1).then(&Mat::translate(spot.x + tpl.h, spot.y)) } else { Mat::translate(spot.x, spot.y) };
                for face in &tpl.faces {
                    if face.side == Side::Back && !duplex {
                        continue;
                    }
                    let page = unit.and_then(|ui| units[ui].pages[face.local_page]);
                    let amt = unit.map(|ui| units[ui].creep[face.local_page]).unwrap_or(0.0);
                    let signed = if job.binding.creep_direction == CreepDirection::Out { -amt } else { amt };
                    let pre = creep_matrix(job.binding.creep_method, signed, face.spine_dir, pw, ph);
                    let m = pre.then(&face.m).then(&p);
                    let trim = p.apply_rect(&face.trim);
                    let to_plate = face.m.then(&p);
                    let face_vec = |m: &Mat| {
                        let (x, y) = m.linear().apply(-face.spine_dir.0, -face.spine_dir.1);
                        (x.round(), y.round())
                    };
                    match face.side {
                        Side::Front => front.push(Placed { page, m, trim, creep: signed, face: face_vec(&to_plate) }),
                        Side::Back => {
                            let to_back = to_plate.then(&back_m);
                            back.push(Placed { page, m: m.then(&back_m), trim: back_m.apply_rect(&trim), creep: signed, face: face_vec(&to_back) })
                        }
                    }
                }
            }
            reading_order(&mut front);
            reading_order(&mut back);
            let mut sigs: Vec<usize> = if binding.folded() { slots.iter().flatten().map(|u| u + 1).collect() } else { Vec::new() };
            sigs.sort();
            sigs.dedup();
            sheets.push(SheetPlan { w, h: sheet_h, front, back, label: tpl.label.clone(), sigs });
        }
    }

    let mut plan = Plan { sheets, bleed, page_w: pw, page_h: ph, flip, notes };

    // ---- manual page map overrides the auto order
    if let Some(map) = &job.pages.map {
        let parsed = parse_map(map)?;
        apply_map(&mut plan, &parsed, job, gutter)?;
    }
    Ok(plan)
}

fn count_sizes(parts: &[usize]) -> std::collections::BTreeMap<usize, usize> {
    let mut m = std::collections::BTreeMap::new();
    for &p in parts {
        *m.entry(p).or_insert(0) += 1;
    }
    m
}

fn describe_parts(parts: &[usize]) -> String {
    let c = count_sizes(parts);
    c.iter().rev().map(|(s, n)| format!("{n} × {s}pp")).collect::<Vec<_>>().join(" + ")
}

/// Roll media: width fixed, length grows. Returns (sheet length, units per sheet, spots).
fn roll_layout(job: &Job, w: f64, bleed: f64, tpl: &Template, gap: f64, units: usize, notes: &mut Vec<String>) -> Result<(f64, usize, Vec<Spot>)> {
    // measure the frame with a tall dummy sheet to get side reserves
    let tall = 1.0e6;
    let fr = frame(job, w, tall, bleed);
    let below = fr.area.y0;
    let above = tall - fr.area.y1;
    let mut best: Option<(f64, bool, usize, usize)> = None; // (length, rot, across, rows)
    for rot in [false, true] {
        if rot && !job.layout.allow_rotation {
            continue;
        }
        let (bw, bh) = if rot { (tpl.h, tpl.w) } else { (tpl.w, tpl.h) };
        let (nx, _) = grid(fr.area.w(), tall, bw, bh, gap);
        if nx == 0 {
            continue;
        }
        let mut rows = units.div_ceil(nx);
        if job.sheet.max_length > 0.0 {
            let usable = job.pt(job.sheet.max_length) - below - above;
            let cap = ((usable + gap + EPS) / (bh + gap)).floor() as usize;
            if cap == 0 {
                continue;
            }
            rows = rows.min(cap);
        }
        let len = rows as f64 * (bh + gap) - gap;
        if best.is_none_or(|b| len < b.0) {
            best = Some((len, rot, nx, rows));
        }
    }
    let (len, rot, nx, rows) = best.with_context(|| format!("{} is wider than the roll", tpl.label))?;
    let h = len + below + above;
    let (bw, bh) = if rot { (tpl.h, tpl.w) } else { (tpl.w, tpl.h) };
    let fr = frame(job, w, h, bleed);
    let mut spots = Vec::new();
    // top row first so a part-filled last sheet starts at the head
    for j in 0..rows {
        for i in 0..nx {
            spots.push(Spot { x: fr.area.x0 + i as f64 * (bw + gap), y: fr.area.y1 - bh - j as f64 * (bh + gap), rot });
        }
    }
    let used_w = nx as f64 * (bw + gap) - gap;
    let dx = (fr.area.w() - used_w) / 2.0;
    for s in spots.iter_mut() {
        s.x += dx;
    }
    notes.push(format!(
        "roll: {nx} across × {rows} rows, length {:.3} {}{}",
        h / job.pt(1.0),
        job.units.label(),
        if rot { " (turned 90°)" } else { "" }
    ));
    Ok((h, nx * rows, spots))
}

// ------------------------------------------------------------- page map --

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub page: Option<usize>,
    /// clockwise degrees: 0, 90, 180, 270
    pub rot: i32,
}

/// One sheet of a manual map: front slots, back slots.
pub type MapSheet = (Vec<Slot>, Vec<Slot>);

fn parse_slot(v: &toml::Value) -> Result<Slot> {
    match v {
        toml::Value::Integer(n) if *n >= 0 => Ok(Slot { page: (*n > 0).then(|| *n as usize - 1), rot: 0 }),
        toml::Value::String(s) => {
            let (p, r) = s.split_once('@').unwrap_or((s, "0"));
            let page: usize = p.trim().parse().with_context(|| format!("bad page `{s}`"))?;
            let rot: i32 = r.trim().parse().with_context(|| format!("bad rotation in `{s}`"))?;
            anyhow::ensure!(rot.rem_euclid(90) == 0, "rotation in `{s}` must be 0, 90, 180 or 270");
            Ok(Slot { page: (page > 0).then(|| page - 1), rot: rot.rem_euclid(360) })
        }
        other => bail!("a page in the map must be a number or \"N@deg\", got {other}"),
    }
}

fn parse_side(v: &toml::Value) -> Result<Vec<Slot>> {
    match v {
        toml::Value::Array(a) => a.iter().map(parse_slot).collect(),
        other => Ok(vec![parse_slot(other)?]),
    }
}

fn depth(v: &toml::Value) -> usize {
    match v {
        toml::Value::Array(a) => 1 + a.first().map(depth).unwrap_or(0),
        _ => 0,
    }
}

/// map = [ signature, ... ]
///   signature = [front, back]                      (one sheet)
///             | [[front, back], [front, back], …]  (several sheets)
///   front/back = [page, …] or a single page
pub fn parse_map(v: &toml::Value) -> Result<Vec<MapSheet>> {
    let sigs = v.as_array().context("pages.map must be an array")?;
    let mut out = Vec::new();
    for (si, sig) in sigs.iter().enumerate() {
        let a = sig.as_array().with_context(|| format!("signature {} in pages.map must be an array", si + 1))?;
        let sheets: Vec<&toml::Value> = if depth(sig) >= 3 { a.iter().collect() } else { vec![sig] };
        for sh in sheets {
            let sides = sh.as_array().context("a sheet must be [front, back]")?;
            anyhow::ensure!(!sides.is_empty() && sides.len() <= 2, "a sheet must be [front] or [front, back]");
            let front = parse_side(&sides[0])?;
            let back = if sides.len() == 2 { parse_side(&sides[1])? } else { Vec::new() };
            out.push((front, back));
        }
    }
    Ok(out)
}

/// The plan's page order in map syntax (copy into `pages.map` and edit).
pub fn format_map(plan: &Plan) -> String {
    let side = |v: &[Placed]| {
        let mut out = String::from("[");
        for (i, p) in v.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&p.page.map(|x| (x + 1).to_string()).unwrap_or_else(|| "0".into()));
        }
        out.push(']');
        out
    };
    let mut s = String::from("map = [\n");
    for sh in &plan.sheets {
        if sh.back.is_empty() {
            s.push_str(&format!("  [ {} ],\n", side(&sh.front)));
        } else {
            s.push_str(&format!("  [ {}, {} ],\n", side(&sh.front), side(&sh.back)));
        }
    }
    s.push(']');
    s
}

fn rotate_in_cell(m: &Mat, rot_cw: i32, pw: f64, ph: f64) -> Mat {
    if rot_cw == 0 {
        return *m;
    }
    let pre = Mat::translate(-pw / 2.0, -ph / 2.0).then(&Mat::rot90(-(rot_cw / 90))).then(&Mat::translate(pw / 2.0, ph / 2.0));
    pre.then(m)
}

fn apply_map(plan: &mut Plan, map: &[MapSheet], job: &Job, gutter: f64) -> Result<()> {
    anyhow::ensure!(!map.is_empty(), "pages.map is empty");
    let auto = std::mem::take(&mut plan.sheets);
    let (pw, ph) = (plan.page_w, plan.page_h);
    let fits = |s: &SheetPlan, (f, b): &MapSheet| f.len() == s.front.len() && (b.is_empty() || b.len() == s.back.len());
    let mut out = Vec::new();
    let mut fallback = 0;
    for (i, ms) in map.iter().enumerate() {
        let base = auto.get(i.min(auto.len().saturating_sub(1)));
        let mut sheet = match base {
            Some(b) if fits(b, ms) => b.clone(),
            _ => {
                fallback += 1;
                manual_grid(job, plan, ms, gutter)?
            }
        };
        for (p, s) in sheet.front.iter_mut().zip(ms.0.iter()) {
            p.page = s.page;
            p.m = rotate_in_cell(&p.m, s.rot, pw, ph);
        }
        if ms.1.is_empty() {
            sheet.back.clear();
        }
        for (p, s) in sheet.back.iter_mut().zip(ms.1.iter()) {
            p.page = s.page;
            p.m = rotate_in_cell(&p.m, s.rot, pw, ph);
        }
        sheet.label = format!("{} (manual)", sheet.label);
        out.push(sheet);
    }
    if fallback > 0 {
        plan.notes.push(format!(
            "page map: {fallback} sheet(s) didn't match the auto layout's slot count and use a plain grid (gutter {:.4} {})",
            gutter / job.pt(1.0), job.units.label()
        ));
    }
    plan.notes.push(format!("page map applied: {} sheet(s)", out.len()));
    plan.sheets = out;
    Ok(())
}

/// A plain grid of page-sized cells for a manual sheet that doesn't match the auto layout.
fn manual_grid(job: &Job, plan: &Plan, ms: &MapSheet, gutter: f64) -> Result<SheetPlan> {
    let n = ms.0.len().max(ms.1.len());
    let w = job.pt(job.sheet.width);
    let h = if job.sheet.height > 0.0 { job.pt(job.sheet.height) } else { plan.sheets.first().map(|s| s.h).unwrap_or(w) };
    let fr = frame(job, w, h, plan.bleed);
    let (pw, ph) = (plan.page_w, plan.page_h);
    let (nx, ny) = grid(fr.area.w(), fr.area.h(), pw, ph, gutter);
    anyhow::ensure!(nx * ny >= n, "page map: {n} pages per side don't fit the sheet ({} fit)", nx * ny);
    let cols = nx.min(n);
    let rows = n.div_ceil(cols);
    let mut spots = Vec::new();
    for j in 0..rows {
        for i in 0..cols {
            if spots.len() < n {
                spots.push(Spot { x: i as f64 * (pw + gutter), y: (rows - 1 - j) as f64 * (ph + gutter), rot: false });
            }
        }
    }
    for s in spots.iter_mut() {
        s.x += fr.area.x0;
        s.y += fr.area.y0;
    }
    align_spots(&mut spots, pw, ph, &fr.area, job);
    let back_m = match plan.flip {
        Flip::Tumble => Mat::mirror_y(h / 2.0),
        _ => Mat::mirror_x(w / 2.0),
    };
    let mut front: Vec<Placed> = spots.iter().map(|s| Placed { page: None, m: Mat::translate(s.x, s.y), trim: Rect::xywh(s.x, s.y, pw, ph), creep: 0.0, face: (0.0, 0.0) }).collect();
    // back slots sit behind front slots; their page is drawn upright in back view
    let mut back: Vec<Placed> = front
        .iter()
        .map(|p| {
            let t = back_m.apply_rect(&p.trim);
            Placed { page: None, m: Mat::translate(t.x0, t.y0), trim: t, creep: 0.0, face: (0.0, 0.0) }
        })
        .collect();
    reading_order(&mut front);
    reading_order(&mut back);
    Ok(SheetPlan { w, h, front, back, label: "manual grid".into(), sigs: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribute_like_pdf_impose() {
        assert_eq!(distribute(2, 4, Fill::Repeat), vec![Some(0), Some(0), Some(1), Some(1)]);
        assert_eq!(distribute(2, 5, Fill::Repeat), vec![Some(0), Some(0), Some(0), Some(1), Some(1)]);
        assert_eq!(distribute(2, 4, Fill::Blank), vec![Some(0), Some(1), None, None]);
    }

    #[test]
    fn decompose_sizes() {
        assert_eq!(decompose(40, 16, &[32, 16, 8, 4]), vec![16, 16, 8]);
        assert_eq!(decompose(12, 32, &[32, 16, 8, 4]), vec![8, 4]);
    }

    #[test]
    fn blanks_before_back_cover() {
        // 10 real pages padded to 12
        let v: Vec<Option<usize>> = (0..12).map(|p| real_page(p, 12, 10, BlankPages::BeforeBackCover)).collect();
        assert_eq!(v[11], Some(9));
        assert_eq!(v[8], Some(8));
        assert_eq!(v[9], None);
        assert_eq!(v[10], None);
    }

    #[test]
    fn mixed_rotation_beats_uniform() {
        // 10 × 7 area, 3 × 2 blocks: uniform 3×3=9, rotated 5×2=10 … mixed should be ≥
        let (spots, _) = arrange(10.0, 7.0, 3.0, 2.0, 0.0, true, true);
        assert!(spots.len() >= 10);
        // classic case: 11 × 8 with 3 × 2: uniform 3×4=12, rotated 5×2=10, mixed 3 cols×4 + strip 2 wide × 8 → 2×...
        let (s2, note) = arrange(11.0, 8.0, 3.0, 2.0, 0.0, true, true);
        assert!(s2.len() >= 14, "got {} {:?}", s2.len(), note);
        // no overlaps
        for (i, a) in s2.iter().enumerate() {
            let (aw, ah) = spot_size(a, 3.0, 2.0);
            let ra = Rect::xywh(a.x, a.y, aw, ah);
            assert!(ra.x1 <= 11.0 + 1e-9 && ra.y1 <= 8.0 + 1e-9);
            for b in &s2[i + 1..] {
                let (bw, bh) = spot_size(b, 3.0, 2.0);
                assert!(!ra.overlaps(&Rect::xywh(b.x, b.y, bw, bh)));
            }
        }
    }

    #[test]
    fn map_parsing() {
        let v: toml::Value = toml::from_str::<toml::Table>("m = [[[1,4],[2,3]]]").unwrap()["m"].clone();
        let m = parse_map(&v).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].0.iter().map(|s| s.page).collect::<Vec<_>>(), vec![Some(0), Some(3)]);
        assert_eq!(m[0].1.iter().map(|s| s.page).collect::<Vec<_>>(), vec![Some(1), Some(2)]);
        let v: toml::Value = toml::from_str::<toml::Table>("m = [[[[4,1],[2,\"3@180\"]], [[8,5],[6,0]]]]").unwrap()["m"].clone();
        let m = parse_map(&v).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].1[1], Slot { page: Some(2), rot: 180 });
        assert_eq!(m[1].1[1].page, None);
    }

    #[test]
    fn creep_matrix_moves_face() {
        // left-bound recto: spine at x = 0, face at x = 100
        for m in [CreepMethod::Shift, CreepMethod::Scale] {
            let c = creep_matrix(m, 5.0, (-1.0, 0.0), 100.0, 140.0);
            let (x, _) = c.apply(100.0, 70.0);
            assert!((x - 95.0).abs() < 1e-9, "{m:?} face moves in by 5");
        }
        // scale keeps the spine edge where it is
        let c = creep_matrix(CreepMethod::Scale, 5.0, (-1.0, 0.0), 100.0, 140.0);
        assert!(c.apply(0.0, 70.0).0.abs() < 1e-9);
        // verso (spine on the right): face at x = 0 moves right
        let c = creep_matrix(CreepMethod::Shift, 5.0, (1.0, 0.0), 100.0, 140.0);
        assert!((c.apply(0.0, 0.0).0 - 5.0).abs() < 1e-9);
    }
}
