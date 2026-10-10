//! Draws a plan into PDF pages: imposed pages (clipped to their bleed,
//! never into a neighbour), trim and bleed marks that never cross any trim or
//! bleed, a colour bar that scales with the sheet, and a slug line.

use crate::sheet::config::{BarScale, Edge, Job, WorkStyle};
use crate::sheet::layout::{frame, Placed, Plan};
use anyhow::Result;
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use crate::pdfcore::{self, cm, fmt, Mat, PageGeom, Rect, EPS};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

pub struct Rendered {
    pub pages_id: ObjectId,
    pub kids: Vec<Object>,
}

struct Shared {
    reg_cs: ObjectId,
    font: ObjectId,
    font_bold: ObjectId,
    bar: Option<(ObjectId, Rect)>,
    xobjects: HashMap<usize, ObjectId>,
}

/// Each page's bleed area on a plate, limited so it never runs into a neighbour.
pub fn clips(items: &[Placed], bleed: f64) -> Vec<Rect> {
    items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let t = it.trim;
            let mut c = t.grow(bleed);
            for (j, o) in items.iter().enumerate() {
                if i == j {
                    continue;
                }
                let o = o.trim;
                let y_overlap = o.y0 < t.y1 - EPS && o.y1 > t.y0 + EPS;
                let x_overlap = o.x0 < t.x1 - EPS && o.x1 > t.x0 + EPS;
                if y_overlap && o.x0 >= t.x1 - EPS {
                    c.x1 = c.x1.min((t.x1 + o.x0) / 2.0);
                }
                if y_overlap && o.x1 <= t.x0 + EPS {
                    c.x0 = c.x0.max((o.x1 + t.x0) / 2.0);
                }
                if x_overlap && o.y0 >= t.y1 - EPS {
                    c.y1 = c.y1.min((t.y1 + o.y0) / 2.0);
                }
                if x_overlap && o.y1 <= t.y0 + EPS {
                    c.y0 = c.y0.max((o.y1 + t.y0) / 2.0);
                }
            }
            c
        })
        .collect()
}

/// A mark from (x, y) going (dx, dy) for `len`, shortened at the first obstacle.
/// None if it starts inside an obstacle or ends up shorter than `min`.
fn ray(x: f64, y: f64, dx: f64, dy: f64, len: f64, min: f64, obstacles: &[Rect], sheet: &Rect) -> Option<[f64; 4]> {
    let mut t = len;
    // stay on the sheet
    if dx > 0.0 { t = t.min(sheet.x1 - x) }
    if dx < 0.0 { t = t.min(x - sheet.x0) }
    if dy > 0.0 { t = t.min(sheet.y1 - y) }
    if dy < 0.0 { t = t.min(y - sheet.y0) }
    for o in obstacles {
        if dx != 0.0 {
            if !(y > o.y0 + EPS && y < o.y1 - EPS) {
                continue;
            }
            if x > o.x0 + EPS && x < o.x1 - EPS {
                return None;
            }
            if dx > 0.0 && o.x0 >= x - EPS { t = t.min(o.x0 - x) }
            if dx < 0.0 && o.x1 <= x + EPS { t = t.min(x - o.x1) }
        } else {
            if !(x > o.x0 + EPS && x < o.x1 - EPS) {
                continue;
            }
            if y > o.y0 + EPS && y < o.y1 - EPS {
                return None;
            }
            if dy > 0.0 && o.y0 >= y - EPS { t = t.min(o.y0 - y) }
            if dy < 0.0 && o.y1 <= y + EPS { t = t.min(y - o.y1) }
        }
    }
    (t >= min - 1e-9).then(|| [x, y, x + dx * t, y + dy * t])
}

/// Trim and bleed marks for one plate.
pub fn marks(job: &Job, items: &[Placed], clip: &[Rect], w: f64, h: f64, keep_out: &[Rect]) -> Vec<[f64; 4]> {
    let mk = &job.marks;
    let (off, len, min) = (job.pt(mk.offset), job.pt(mk.length), job.pt(mk.min_length));
    let sheet = Rect::new(0.0, 0.0, w, h);
    let mut obstacles: Vec<Rect> = clip.iter().map(|c| c.grow(off - EPS)).collect();
    obstacles.extend(keep_out.iter().copied());
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |s: Option<[f64; 4]>, out: &mut Vec<[f64; 4]>| {
        if let Some(s) = s {
            let key = s.map(|v| (v * 100.0).round() as i64);
            if seen.insert(key) {
                out.push(s);
            }
        }
    };
    for (it, c) in items.iter().zip(clip) {
        let t = it.trim;
        let mut lines: Vec<(f64, f64, f64, f64)> = Vec::new(); // start x, y, dir x, dir y
        if mk.trim {
            for x in [t.x0, t.x1] {
                lines.push((x, c.y1 + off, 0.0, 1.0));
                lines.push((x, c.y0 - off, 0.0, -1.0));
            }
            for y in [t.y0, t.y1] {
                lines.push((c.x1 + off, y, 1.0, 0.0));
                lines.push((c.x0 - off, y, -1.0, 0.0));
            }
        }
        if mk.bleed {
            let mut xs = Vec::new();
            if c.x0 < t.x0 - EPS { xs.push(c.x0) }
            if c.x1 > t.x1 + EPS { xs.push(c.x1) }
            for x in xs {
                lines.push((x, c.y1 + off, 0.0, 1.0));
                lines.push((x, c.y0 - off, 0.0, -1.0));
            }
            let mut ys = Vec::new();
            if c.y0 < t.y0 - EPS { ys.push(c.y0) }
            if c.y1 > t.y1 + EPS { ys.push(c.y1) }
            for y in ys {
                lines.push((c.x1 + off, y, 1.0, 0.0));
                lines.push((c.x0 - off, y, -1.0, 0.0));
            }
        }
        for (x, y, dx, dy) in lines {
            push(ray(x, y, dx, dy, len, min, &obstacles, &sheet), &mut out);
        }
    }
    out
}

/// Creep marks: at head and foot of every crept page, a short line where its
/// face will trim after creep. Same placement rules as trim marks.
pub fn creep_marks(job: &Job, items: &[Placed], clip: &[Rect], w: f64, h: f64, keep_out: &[Rect]) -> Vec<[f64; 4]> {
    let mk = &job.marks;
    let (off, len, min) = (job.pt(mk.offset), job.pt(mk.length), job.pt(mk.min_length));
    let sheet = Rect::new(0.0, 0.0, w, h);
    let mut obstacles: Vec<Rect> = clip.iter().map(|c| c.grow(off - EPS)).collect();
    obstacles.extend(keep_out.iter().copied());
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (it, c) in items.iter().zip(clip) {
        if it.creep.abs() < 0.01 || it.face == (0.0, 0.0) {
            continue;
        }
        let t = it.trim;
        let (fx, fy) = it.face;
        let rays: Vec<(f64, f64, f64, f64)> = if fx != 0.0 {
            // face is left or right: creep line is vertical
            let x = if fx > 0.0 { t.x1 - it.creep } else { t.x0 + it.creep };
            vec![(x, c.y1 + off, 0.0, 1.0), (x, c.y0 - off, 0.0, -1.0)]
        } else {
            let y = if fy > 0.0 { t.y1 - it.creep } else { t.y0 + it.creep };
            vec![(c.x1 + off, y, 1.0, 0.0), (c.x0 - off, y, -1.0, 0.0)]
        };
        for (x, y, dx, dy) in rays {
            if let Some(s) = ray(x, y, dx, dy, len, min, &obstacles, &sheet) {
                if seen.insert(s.map(|v| (v * 100.0).round() as i64)) {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// Approximate Helvetica-Bold advance width.
fn text_width_bold(s: &str, size: f64) -> f64 {
    let w: f64 = s
        .chars()
        .map(|c| match c {
            '0'..='9' => 556.0,
            'A' | 'B' | 'C' | 'D' | 'H' | 'K' | 'N' | 'R' | 'U' | 'V' | 'X' | 'Y' => 722.0,
            'E' | 'P' | 'S' | 'T' | 'Z' => 667.0,
            'F' | 'L' => 611.0,
            'G' | 'O' | 'Q' => 778.0,
            'I' => 278.0,
            'J' => 556.0,
            'M' => 833.0,
            'W' => 944.0,
            '-' | '/' => 333.0,
            ' ' | '.' | ',' => 278.0,
            'a'..='z' => 556.0,
            _ => 600.0,
        })
        .sum();
    w / 1000.0 * size
}

const BUILTIN_BAR: [[f64; 4]; 14] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
    [0.5, 0.0, 0.0, 0.0],
    [0.0, 0.5, 0.0, 0.0],
    [0.0, 0.0, 0.5, 0.0],
    [0.0, 0.0, 0.0, 0.5],
    [1.0, 1.0, 0.0, 0.0],
    [1.0, 0.0, 1.0, 0.0],
    [0.0, 1.0, 1.0, 0.0],
    [0.5, 0.4, 0.4, 0.0],
    [0.0, 0.0, 0.0, 0.25],
    [0.0, 0.0, 0.0, 0.75],
];

fn colorbar(job: &Job, bar: &Rect, shared: &Shared, out: &mut String) {
    let cb = &job.colorbar;
    let vertical = matches!(cb.position, Edge::Left | Edge::Right);
    let (long, thick) = if vertical { (bar.h(), bar.w()) } else { (bar.w(), bar.h()) };
    let used = if cb.length > 0.0 { job.pt(cb.length).min(long) } else { long };
    let u0 = (long - used) / 2.0;
    // band-local (u along the bar, v across) → sheet
    let to_sheet = if vertical { Mat::rot90(1).then(&Mat::translate(bar.x1, bar.y0)) } else { Mat::translate(bar.x0, bar.y0) };
    let _ = writeln!(out, "q {} {} 0 {} {} re W n", cm(&to_sheet), fmt(u0), fmt(used), fmt(thick));
    match shared.bar {
        None => {
            let s = thick;
            let count = (used / s).floor() as usize;
            let start = u0 + (used - count as f64 * s) / 2.0;
            for k in 0..count {
                let [c, m, y, kk] = BUILTIN_BAR[k % BUILTIN_BAR.len()];
                let _ = writeln!(out, "{} {} {} {} k {} 0 {} {} re f", fmt(c), fmt(m), fmt(y), fmt(kk), fmt(start + k as f64 * s), fmt(s), fmt(s));
            }
        }
        Some((_, bb)) => {
            let place = |sx: f64, sy: f64, du: f64, dv: f64, out: &mut String| {
                let m = Mat::translate(-bb.x0, -bb.y0).then(&Mat::scale(sx, sy)).then(&Mat::translate(du, dv));
                let _ = writeln!(out, "q {} /CB Do Q", cm(&m));
            };
            match cb.scale {
                BarScale::Tile => {
                    let mut sc = thick / bb.h();
                    if bb.w() * sc > used {
                        sc = used / bb.w();
                    }
                    let tw = bb.w() * sc;
                    let count = ((used + EPS) / tw).floor().max(1.0) as usize;
                    let start = u0 + (used - count as f64 * tw) / 2.0;
                    let dv = (thick - bb.h() * sc) / 2.0;
                    for k in 0..count {
                        place(sc, sc, start + k as f64 * tw, dv, out);
                    }
                }
                BarScale::Stretch => place(used / bb.w(), thick / bb.h(), u0, 0.0, out),
                BarScale::Fit => {
                    let sc = (used / bb.w()).min(thick / bb.h());
                    place(sc, sc, u0 + (used - bb.w() * sc) / 2.0, (thick - bb.h() * sc) / 2.0, out);
                }
                BarScale::None => place(1.0, 1.0, u0 + (used - bb.w()) / 2.0, (thick - bb.h()) / 2.0, out),
            }
        }
    }
    out.push_str("Q\n");
}

struct PlateCtx<'a> {
    job: &'a Job,
    plan: &'a Plan,
    geoms: &'a [PageGeom],
    name: &'a str,
}

#[allow(clippy::too_many_arguments)]
fn plate(doc: &mut Document, ctx: &PlateCtx, shared: &mut Shared, items: &[Placed], w: f64, h: f64, is_back: bool, slug: &str, sig_text: &str, pages_id: ObjectId) -> Result<ObjectId> {
    let job = ctx.job;
    let plan = ctx.plan;
    let clip = clips(items, plan.bleed);
    let mut out = String::new();
    let mut xobj = Dictionary::new();

    // pages
    for (it, c) in items.iter().zip(&clip) {
        let Some(p) = it.page else { continue };
        let g = &ctx.geoms[p];
        let id = match shared.xobjects.get(&p) {
            Some(&id) => id,
            None => {
                let id = pdfcore::page_to_xobject(doc, g.id, &g.media)?;
                shared.xobjects.insert(p, id);
                id
            }
        };
        let name = format!("P{}", p + 1);
        xobj.set(name.as_bytes().to_vec(), Object::Reference(id));
        let (dw, dh) = g.display_size();
        let centre = Mat::translate((plan.page_w - dw) / 2.0, (plan.page_h - dh) / 2.0);
        let m = g.display_matrix().then(&centre).then(&it.m);
        let _ = writeln!(out, "q {} {} {} {} re W n {} /{} Do Q", fmt(c.x0), fmt(c.y0), fmt(c.w()), fmt(c.h()), cm(&m), name);
    }

    let fr = frame(job, w, h, plan.bleed);
    let bar_on = job.colorbar.enabled && (!is_back || job.colorbar.back);
    let mut keep_out = Vec::new();
    if let (true, Some(b)) = (bar_on, fr.bar) {
        keep_out.push(b);
    }

    // marks
    let marks_on = (job.marks.trim || job.marks.bleed) && (!is_back || job.marks.back);
    if marks_on {
        let segs = marks(job, items, &clip, w, h, &keep_out);
        if !segs.is_empty() {
            let _ = writeln!(out, "q /CSreg CS 1 SCN {} w 0 J", fmt(job.marks.line_width));
            for s in segs {
                let _ = writeln!(out, "{} {} m {} {} l", fmt(s[0]), fmt(s[1]), fmt(s[2]), fmt(s[3]));
            }
            out.push_str("S Q\n");
        }
    }

    // creep marks (dashed)
    if job.marks.creep && (!is_back || job.marks.back) {
        let segs = creep_marks(job, items, &clip, w, h, &keep_out);
        if !segs.is_empty() {
            let _ = writeln!(out, "q /CSreg CS 1 SCN {} w 0 J [2 1.5] 0 d", fmt(job.marks.line_width));
            for s in segs {
                let _ = writeln!(out, "{} {} m {} {} l", fmt(s[0]), fmt(s[1]), fmt(s[2]), fmt(s[3]));
            }
            out.push_str("S Q\n");
        }
    }

    // colour bar
    if let (true, Some(b)) = (bar_on, fr.bar) {
        colorbar(job, &b, shared, &mut out);
        if let Some((id, _)) = shared.bar {
            xobj.set("CB", Object::Reference(id));
        }
    }

    // signature text mark, left and right edges, centred
    let sm = &job.marks.signature;
    if sm.enabled && !sig_text.is_empty() {
        let size = sm.size;
        let tw = text_width_bold(sig_text, size);
        let cap = 0.72 * size;
        // distance from each paper edge to the text
        let (ml, mr) = match sm.edge_distance {
            Some(d) => (job.pt(d), job.pt(d)),
            None => {
                let (l, r, _, _) = crate::sheet::layout::margins(job);
                (l + job.pt(sm.inset), r + job.pt(sm.inset))
            }
        };
        let inset = 0.0;
        let txt = pdfcore::pdf_text(sig_text);
        let mut boxes = Vec::new();
        if sm.left {
            let x = ml + inset;
            let (tm, bx) = if sm.rotate {
                // reads bottom → top, glyphs toward the sheet edge
                (format!("0 1 -1 0 {} {}", fmt(x + cap), fmt(h / 2.0 - tw / 2.0)), Rect::new(x, h / 2.0 - tw / 2.0, x + cap, h / 2.0 + tw / 2.0))
            } else {
                (format!("1 0 0 1 {} {}", fmt(x), fmt(h / 2.0 - cap / 2.0)), Rect::new(x, h / 2.0 - cap / 2.0, x + tw, h / 2.0 + cap / 2.0))
            };
            let _ = writeln!(out, "q /CSreg cs 1 scn BT /F2 {} Tf {} Tm ({}) Tj ET Q", fmt(size), tm, txt);
            boxes.push(bx);
        }
        if sm.right {
            let x = w - mr - inset;
            let (tm, bx) = if sm.rotate {
                // reads top → bottom
                (format!("0 -1 1 0 {} {}", fmt(x - cap), fmt(h / 2.0 + tw / 2.0)), Rect::new(x - cap, h / 2.0 - tw / 2.0, x, h / 2.0 + tw / 2.0))
            } else {
                (format!("1 0 0 1 {} {}", fmt(x - tw), fmt(h / 2.0 - cap / 2.0)), Rect::new(x - tw, h / 2.0 - cap / 2.0, x, h / 2.0 + cap / 2.0))
            };
            let _ = writeln!(out, "q /CSreg cs 1 scn BT /F2 {} Tf {} Tm ({}) Tj ET Q", fmt(size), tm, txt);
            boxes.push(bx);
        }
        for b in boxes {
            if clip.iter().any(|c| c.overlaps(&b)) {
                log::warn!("signature mark \"{sig_text}\" overlaps page bleed — reduce marks.signature.edge_distance (or inset) / size, or widen the margins");
            }
        }
    }

    // slug
    if job.marks.slug && (!is_back || job.marks.back) {
        let size = 6.0;
        let top_margin = job.pt(job.sheet.margin_top);
        let y = if top_margin >= size + 2.0 { h - top_margin / 2.0 - size / 3.0 } else { h - size - 1.0 };
        let x = job.pt(job.sheet.margin_left).max(4.0);
        let _ = writeln!(
            out,
            "q /CSreg cs 1 scn BT /F1 {} Tf {} {} Td ({}) Tj ET Q",
            fmt(size), fmt(x), fmt(y), pdfcore::pdf_text(&format!("{} | {}", ctx.name, slug))
        );
    }

    let mut st = Stream::new(Dictionary::new(), out.into_bytes());
    let _ = st.compress();
    let content = doc.add_object(st);
    let media = Rect::new(0.0, 0.0, w, h);
    let page = dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => pdfcore::rect_obj(&media),
        "TrimBox" => pdfcore::rect_obj(&media),
        "Resources" => dictionary! {
            "XObject" => xobj,
            "ColorSpace" => dictionary! { "CSreg" => shared.reg_cs },
            "Font" => dictionary! { "F1" => shared.font, "F2" => shared.font_bold },
        },
        "Contents" => content,
    };
    Ok(doc.add_object(page))
}

pub fn render(doc: &mut Document, job: &Job, plan: &Plan, geoms: &[PageGeom], name: &str, job_dir: &Path) -> Result<Rendered> {
    let pages_id = doc.new_object_id();
    let reg_cs = pdfcore::add_separation(doc, "All", [100.0, 100.0, 100.0, 100.0]);
    let font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let bar = if job.colorbar.enabled && !job.colorbar.file.trim().is_empty() {
        let p = Path::new(job.colorbar.file.trim());
        let p = if p.is_absolute() { p.to_path_buf() } else { job_dir.join(p) };
        Some(pdfcore::import_page(doc, &p, job.colorbar.page)?)
    } else {
        None
    };
    let font_bold = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica-Bold",
        "Encoding" => "WinAnsiEncoding",
    });
    let mut shared = Shared { reg_cs, font, font_bold, bar, xobjects: HashMap::new() };
    let ctx = PlateCtx { job, plan, geoms, name };
    let ws = job.press.work_style;
    let n = plan.sheets.len();
    let mut kids = Vec::new();
    for (i, sh) in plan.sheets.iter().enumerate() {
        let tag = format!("sheet {}/{} | {} {} | {}", i + 1, n, ws.name(), job.binding.style.name(), sh.label);
        let sm = &job.marks.signature;
        let sig_list = if sh.sigs.is_empty() { (i + 1).to_string() } else { sh.sigs.iter().map(|s| s.to_string()).collect::<Vec<_>>().join("+") };
        let label = |side: &str| -> String {
            sm.text
                .replace("{sheet}", &(i + 1).to_string())
                .replace("{sheets}", &n.to_string())
                .replace("{sig}", &sig_list)
                .replace("{side}", side)
        };
        let (lab_front, lab_back, lab_turn) = (label(&sm.front), label(&sm.back), label(&format!("{}{}", sm.front, sm.back)));
        match ws {
            WorkStyle::Single => {
                kids.push(plate(doc, &ctx, &mut shared, &sh.front, sh.w, sh.h, false, &format!("{tag} | front"), &lab_front, pages_id)?);
            }
            WorkStyle::Sheetwise | WorkStyle::Perfect => {
                kids.push(plate(doc, &ctx, &mut shared, &sh.front, sh.w, sh.h, false, &format!("{tag} | front"), &lab_front, pages_id)?);
                kids.push(plate(doc, &ctx, &mut shared, &sh.back, sh.w, sh.h, true, &format!("{tag} | back"), &lab_back, pages_id)?);
            }
            WorkStyle::Workturn | WorkStyle::Worktumble => {
                let mut all = sh.front.clone();
                all.extend(sh.back.iter().cloned());
                let label = if ws == WorkStyle::Workturn { "work & turn" } else { "work & tumble" };
                let id = plate(doc, &ctx, &mut shared, &all, sh.w, sh.h, false, &format!("{tag} | {label}"), &lab_turn, pages_id)?;
                kids.push(id);
                if job.press.output_back_plate {
                    kids.push(id);
                }
            }
        }
    }
    Ok(Rendered { pages_id, kids: kids.into_iter().map(Object::Reference).collect() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(x: f64, y: f64, w: f64, h: f64) -> Placed {
        Placed { page: Some(0), m: Mat::translate(x, y), trim: Rect::xywh(x, y, w, h), creep: 0.0, face: (0.0, 0.0) }
    }

    #[test]
    fn bleed_stops_at_neighbours() {
        // two pages butted (spine) and one 18pt away
        let items = vec![item(0.0, 0.0, 100.0, 100.0), item(100.0, 0.0, 100.0, 100.0), item(218.0, 0.0, 100.0, 100.0)];
        let c = clips(&items, 9.0);
        assert_eq!(c[0].x1, 100.0); // no bleed across the fold
        assert_eq!(c[1].x0, 100.0);
        assert_eq!(c[1].x1, 209.0); // 18 gap → full 9 bleed each
        assert_eq!(c[2].x0, 209.0);
        assert_eq!(c[0].x0, -9.0);
    }

    #[test]
    fn marks_never_cross_trim_or_bleed() {
        let job = Job::default();
        let items = vec![item(100.0, 100.0, 100.0, 100.0), item(200.0, 100.0, 100.0, 100.0), item(100.0, 218.0, 100.0, 100.0)];
        let c = clips(&items, 9.0);
        let segs = marks(&job, &items, &c, 600.0, 600.0, &[]);
        assert!(!segs.is_empty());
        let off = job.pt(job.marks.offset);
        for s in &segs {
            let seg = Rect::new(s[0], s[1], s[2], s[3]);
            for cl in &c {
                let keep_out = cl.grow(off - 2.0 * EPS);
                // a segment may touch the boundary but never run inside
                let inside = seg.x0.max(keep_out.x0) < seg.x1.min(keep_out.x1) - EPS
                    && seg.y0.max(keep_out.y0) < seg.y1.min(keep_out.y1) - EPS
                    || (seg.w() < EPS && seg.x0 > keep_out.x0 + EPS && seg.x0 < keep_out.x1 - EPS && seg.y0 < keep_out.y1 - EPS && seg.y1 > keep_out.y0 + EPS)
                    || (seg.h() < EPS && seg.y0 > keep_out.y0 + EPS && seg.y0 < keep_out.y1 - EPS && seg.x0 < keep_out.x1 - EPS && seg.x1 > keep_out.x0 + EPS);
                assert!(!inside, "mark {s:?} crosses {cl:?}");
            }
        }
    }
}
