//! Single-row step-and-repeat imposition.
//!
//! Every source page is turned into a Form XObject clipped to its bleed box,
//! and new sheet pages place those XObjects side by side. The original page
//! content is reused as-is (no rasterising, no re-encoding of fonts/images),
//! which keeps it fast and lossless.

use crate::row::config::{
    Config, UnderlayExtent, HAlign, MarkAnchor, Order, Orientation, Remainder, RotateDirection, VAlign,
};
use anyhow::{anyhow, bail, Context, Result};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

const EPS: f64 = 0.01; // points

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    fn new(a: f64, b: f64, c: f64, d: f64) -> Self {
        Rect { x0: a.min(c), y0: b.min(d), x1: a.max(c), y1: b.max(d) }
    }
    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }
    fn intersect(&self, o: &Rect) -> Rect {
        let r = Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) };
        if r.w() <= 0.0 || r.h() <= 0.0 { *self } else { r }
    }
    fn grow(&self, d: f64) -> Rect {
        Rect { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }
    fn approx_eq(&self, o: &Rect) -> bool {
        (self.x0 - o.x0).abs() < EPS && (self.y0 - o.y0).abs() < EPS
            && (self.x1 - o.x1).abs() < EPS && (self.y1 - o.y1).abs() < EPS
    }
}

/// Geometry of one source page as it will be placed.
#[derive(Debug, Clone)]
pub struct PageInfo {
    pub id: ObjectId,
    pub bleed: Rect,
    pub trim: Rect,
    /// Effective clockwise rotation (0/90/180/270): /Rotate + forced orientation.
    pub rotate: i32,
    /// Displayed bleed size after rotation.
    pub disp_w: f64,
    pub disp_h: f64,
}

#[derive(Debug, Clone)]
pub struct ImposeReport {
    pub across: usize,
    pub sheets: usize,
    pub pages: usize,
    pub sheet_w_pt: f64,
    pub sheet_h_pt: f64,
    /// First bleed to last bleed.
    pub row_w_pt: f64,
    pub slot_w_pt: f64,
}

// ---------------------------------------------------------------- reading --

/// Look up a (possibly inherited) page attribute, resolving references.
fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut id = page;
    for _ in 0..64 {
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(v) = dict.get(key) {
            return doc.dereference(v).ok().map(|(_, o)| o.clone());
        }
        id = dict.get(b"Parent").and_then(Object::as_reference).ok()?;
    }
    None
}

fn num(doc: &Document, o: &Object) -> Option<f64> {
    let o = doc.dereference(o).ok()?.1;
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

fn page_box(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Rect> {
    let arr = inherited(doc, page, key)?;
    let a = arr.as_array().ok()?;
    if a.len() != 4 {
        return None;
    }
    let v: Vec<f64> = a.iter().filter_map(|o| num(doc, o)).collect();
    if v.len() != 4 {
        return None;
    }
    let r = Rect::new(v[0], v[1], v[2], v[3]);
    (r.w() > 0.0 && r.h() > 0.0).then_some(r)
}

pub fn page_info(doc: &Document, id: ObjectId, cfg: &Config) -> Result<PageInfo> {
    let media = page_box(doc, id, b"MediaBox").unwrap_or(Rect::new(0.0, 0.0, 612.0, 792.0));
    let crop = page_box(doc, id, b"CropBox").map(|c| c.intersect(&media)).unwrap_or(media);
    let trim_raw = page_box(doc, id, b"TrimBox");
    let bleed_raw = page_box(doc, id, b"BleedBox");

    let inset = cfg.pt(cfg.bleed.no_trimbox_inset);
    let trim = match trim_raw {
        Some(t) => t,
        None if inset > 0.0 => crop.grow(-inset),
        None => crop,
    };
    let default_bleed = cfg.pt(cfg.bleed.default_bleed);
    let bleed = match bleed_raw {
        // Some apps write BleedBox == TrimBox when no bleed was exported.
        Some(b) if default_bleed > 0.0 && b.approx_eq(&trim) => trim.grow(default_bleed).intersect(&media),
        Some(b) => b.intersect(&crop),
        None if default_bleed > 0.0 => trim.grow(default_bleed).intersect(&media),
        None if trim_raw.is_none() && inset > 0.0 => crop,
        None => crop, // PDF spec: BleedBox defaults to CropBox
    };

    let mut rotate = inherited(doc, id, b"Rotate")
        .and_then(|o| num(doc, &o))
        .map(|r| r as i32)
        .unwrap_or(0);
    rotate = ((rotate % 360) + 360) % 360 / 90 * 90;

    let (mut w, mut h) = if rotate % 180 == 0 { (bleed.w(), bleed.h()) } else { (bleed.h(), bleed.w()) };
    let turn = match cfg.layout.orientation {
        Orientation::Auto => false,
        Orientation::Portrait => w > h + EPS,
        Orientation::Landscape => h > w + EPS,
    };
    if turn {
        rotate = (rotate + if cfg.layout.rotate_direction == RotateDirection::Cw { 90 } else { 270 }) % 360;
        std::mem::swap(&mut w, &mut h);
    }
    Ok(PageInfo { id, bleed, trim, rotate, disp_w: w, disp_h: h })
}

// --------------------------------------------------------------- geometry --

/// Matrix [a b c d e f] that rotates `bleed` clockwise by `rot` and puts the
/// lower-left corner of the displayed result at (tx, ty).
pub fn place_matrix(bleed: &Rect, rot: i32, tx: f64, ty: f64) -> [f64; 6] {
    let (x0, y0, w, h) = (bleed.x0, bleed.y0, bleed.w(), bleed.h());
    let m = match rot {
        90 => [0.0, -1.0, 1.0, 0.0, -y0, x0 + w],
        180 => [-1.0, 0.0, 0.0, -1.0, x0 + w, y0 + h],
        270 => [0.0, 1.0, -1.0, 0.0, y0 + h, -x0],
        _ => [1.0, 0.0, 0.0, 1.0, -x0, -y0],
    };
    [m[0], m[1], m[2], m[3], m[4] + tx, m[5] + ty]
}

fn transform_rect(m: &[f64; 6], r: &Rect) -> Rect {
    let pts = [(r.x0, r.y0), (r.x1, r.y0), (r.x0, r.y1), (r.x1, r.y1)];
    let t: Vec<(f64, f64)> = pts.iter().map(|&(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])).collect();
    let xs = t.iter().map(|p| p.0);
    let ys = t.iter().map(|p| p.1);
    Rect {
        x0: xs.clone().fold(f64::MAX, f64::min),
        x1: xs.fold(f64::MIN, f64::max),
        y0: ys.clone().fold(f64::MAX, f64::min),
        y1: ys.fold(f64::MIN, f64::max),
    }
}

/// How many copies of width `slot` fit across `usable` with `gap` between them.
pub fn fit_count(usable: f64, slot: f64, gap: f64, max_across: usize) -> usize {
    if slot <= 0.0 || usable + EPS < slot {
        return 0;
    }
    let n = ((usable + gap + EPS) / (slot + gap)).floor() as usize;
    if max_across > 0 { n.min(max_across) } else { n }
}

/// Fill `slots` positions with the indices `0..k`.
///
/// k >= slots: each page once, in order (caller chunks pages by `slots`).
/// k <  slots: every page repeated slots/k times; any leftover slots get one
/// more copy of the first pages (Fill) or stay empty (Blank).
pub fn distribute(k: usize, slots: usize, order: Order, remainder: Remainder) -> Vec<usize> {
    if k == 0 || slots == 0 {
        return vec![];
    }
    if k >= slots {
        return (0..slots).collect();
    }
    let base = slots / k;
    let extra = if remainder == Remainder::Fill { slots % k } else { 0 };
    let counts: Vec<usize> = (0..k).map(|i| base + usize::from(i < extra)).collect();
    let mut out = Vec::with_capacity(slots);
    match order {
        Order::Grouped => {
            for (i, &c) in counts.iter().enumerate() {
                out.extend(std::iter::repeat_n(i, c));
            }
        }
        Order::Collated => {
            let max = *counts.iter().max().unwrap();
            for rep in 0..max {
                for (i, &c) in counts.iter().enumerate() {
                    if rep < c {
                        out.push(i);
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- writing --

fn fmt(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

fn rect_obj(r: &Rect) -> Object {
    Object::Array(vec![
        Object::Real(r.x0 as f32),
        Object::Real(r.y0 as f32),
        Object::Real(r.x1 as f32),
        Object::Real(r.y1 as f32),
    ])
}

/// Add a Separation (spot colour) space whose alternate is the given CMYK percentages.
fn add_separation(doc: &mut Document, name: &str, cmyk: [f64; 4]) -> ObjectId {
    let alt = cmyk.map(|v| Object::Real((v / 100.0) as f32));
    let tint_fn = dictionary! {
        "FunctionType" => 2,
        "Domain" => vec![0.into(), 1.into()],
        "C0" => vec![0.into(), 0.into(), 0.into(), 0.into()],
        "C1" => alt.to_vec(),
        "N" => 1,
    };
    doc.add_object(Object::Array(vec![
        Object::Name(b"Separation".to_vec()),
        Object::Name(name.as_bytes().to_vec()),
        Object::Name(b"DeviceCMYK".to_vec()),
        Object::Dictionary(tint_fn),
    ]))
}

/// Convert a page into a Form XObject clipped to its bleed box.
fn page_to_xobject(doc: &mut Document, info: &PageInfo, compress: bool) -> Result<ObjectId> {
    let content = doc.get_page_content(info.id);
    let resources = inherited(doc, info.id, b"Resources").unwrap_or_else(|| Object::Dictionary(Dictionary::new()));
    // Keep the original indirect reference if there is one (avoids duplicating big dicts).
    let resources = match doc.get_dictionary(info.id).ok().and_then(|d| d.get(b"Resources").ok()) {
        Some(r @ Object::Reference(_)) => r.clone(),
        _ => resources,
    };
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "FormType" => 1,
        "BBox" => rect_obj(&info.bleed),
        "Resources" => resources,
    };
    if let Ok(group) = doc.get_dictionary(info.id).and_then(|d| d.get(b"Group")) {
        dict.set("Group", group.clone());
    }
    let mut stream = Stream::new(dict, content);
    if compress {
        let _ = stream.compress();
    }
    Ok(doc.add_object(stream))
}

/// Impose `input` into `output` according to `cfg`.
pub fn impose_file(input: &Path, output: &Path, cfg: &Config) -> Result<ImposeReport> {
    let mut doc = Document::load(input).with_context(|| format!("opening {}", input.display()))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|e| anyhow!("PDF is password protected: {e}"))?;
    }
    let report = impose_doc(&mut doc, cfg)?;

    let tmp = output.with_extension("pdf.part");
    {
        let file = std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        let mut w = std::io::BufWriter::new(file);
        if cfg.output.object_streams {
            let opts = lopdf::SaveOptions::builder().use_object_streams(true).use_xref_streams(true).build();
            doc.save_with_options(&mut w, opts)?;
        } else {
            doc.save_to(&mut w)?;
        }
        use std::io::Write;
        w.flush()?;
    }
    std::fs::rename(&tmp, output).with_context(|| format!("writing {}", output.display()))?;
    Ok(report)
}

/// Rebuild `doc` in place: its page tree is replaced by the imposed sheets.
pub fn impose_doc(doc: &mut Document, cfg: &Config) -> Result<ImposeReport> {
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    if page_ids.is_empty() {
        bail!("PDF has no pages");
    }
    let infos: Vec<PageInfo> = page_ids.iter().map(|&id| page_info(doc, id, cfg)).collect::<Result<_>>()?;

    // ---- how many across
    let sheet_w = cfg.pt(cfg.sheet.width);
    let ml = cfg.pt(cfg.sheet.margin_left);
    let mr = cfg.pt(cfg.sheet.margin_right);
    let mt = cfg.pt(cfg.sheet.margin_top);
    let mb = cfg.pt(cfg.sheet.margin_bottom);
    let gap = cfg.pt(cfg.layout.gap);
    let mark_w = if cfg.mark.enabled { cfg.pt(cfg.mark.width) } else { 0.0 };
    let mark_h = if cfg.mark.enabled { cfg.pt(cfg.mark.height) } else { 0.0 };
    let mark_dx = if cfg.mark.enabled { cfg.pt(cfg.mark.offset_x) } else { 0.0 };
    // Room the eye mark needs to the left of the first bleed.
    let mark_space = if cfg.mark.enabled && mark_w > 0.0 && mark_h > 0.0 { mark_w + mark_dx } else { 0.0 };
    // Only counted against sheet.width when reserve_space = true; otherwise
    // sheet.width is purely the first-bleed-to-last-bleed row and the mark is
    // added outside it afterwards.
    let reserve = if cfg.mark.reserve_space { mark_space } else { 0.0 };

    let slot_w = infos.iter().map(|p| p.disp_w).fold(0.0, f64::max);
    let usable = sheet_w - ml - mr - reserve;
    let across = fit_count(usable, slot_w, gap, cfg.layout.max_across);
    if across == 0 {
        let u = |pt: f64| pt / cfg.pt(1.0);
        bail!(
            "page bleed width {:.4} does not fit in usable row width {:.4} ({:?}) — sheet.width {:.4} minus margins",
            u(slot_w), u(usable), cfg.units, cfg.sheet.width
        );
    }

    // ---- which pages go on which sheet
    let k = infos.len();
    let sheets: Vec<Vec<usize>> = if k <= across {
        vec![distribute(k, across, cfg.layout.order, cfg.layout.remainder)]
    } else {
        (0..k)
            .collect::<Vec<_>>()
            .chunks(across)
            .map(|chunk| {
                distribute(chunk.len(), across, cfg.layout.order, cfg.layout.remainder)
                    .into_iter()
                    .map(|i| chunk[i])
                    .collect()
            })
            .collect()
    };

    // ---- build
    let pages_id = doc.new_object_id();
    let mut xobjects: HashMap<usize, ObjectId> = HashMap::new();
    let mut kids = Vec::with_capacity(sheets.len());
    let (mut last_w, mut last_h, mut last_row) = (0.0, 0.0, 0.0);

    // Spot colours (Separation colour spaces) and one shared overprint ExtGState.
    let mark_on = cfg.mark.enabled && mark_w > 0.0 && mark_h > 0.0;
    let spot = &cfg.mark.spot;
    let under = &cfg.mark.underlay;
    let spot_cs = (mark_on && spot.enabled).then(|| add_separation(doc, &spot.name, spot.cmyk));
    let under_cs = (mark_on && under.enabled).then(|| add_separation(doc, &under.name, under.cmyk));
    let gs_op = doc.add_object(dictionary! {
        "Type" => "ExtGState",
        "OP" => true,
        "op" => true,
        "OPM" => 1,
    });

    for seq in &sheets {
        let row_h = seq.iter().map(|&i| infos[i].disp_h).fold(0.0, f64::max);
        let sheet_h = if cfg.sheet.height > 0.0 { cfg.pt(cfg.sheet.height) } else { row_h + mt + mb };
        let n = seq.len();
        let row_w = n as f64 * slot_w + (n.saturating_sub(1)) as f64 * gap;
        // shrink: sheet = [margin][mark][first bleed ... last bleed][margin]
        // fixed: sheet = sheet.width, row aligned inside it after the mark area
        let shrink = cfg.sheet.shrink_width_to_content;
        let lead = if shrink { mark_space.max(reserve) } else { reserve };
        let this_w = if shrink { ml + lead + row_w + mr } else { sheet_w };
        let avail_x = ml + lead;
        let avail_w = this_w - ml - lead - mr;
        let x_start = match cfg.sheet.align {
            HAlign::Left => avail_x,
            HAlign::Center => avail_x + (avail_w - row_w) / 2.0,
            HAlign::Right => avail_x + avail_w - row_w,
        };
        let inner_h = sheet_h - mt - mb;

        let mut content = String::new();
        let mut xobj_dict = Dictionary::new();
        let mut first: Option<(Rect, Rect)> = None; // (placed bleed, placed trim)

        for (slot, &pi) in seq.iter().enumerate() {
            let info = &infos[pi];
            let xo = match xobjects.get(&pi) {
                Some(&id) => id,
                None => {
                    let id = page_to_xobject(doc, info, cfg.output.compress)?;
                    xobjects.insert(pi, id);
                    id
                }
            };
            let name = format!("P{}", pi + 1);
            xobj_dict.set(name.as_bytes().to_vec(), Object::Reference(xo));

            let tx = x_start + slot as f64 * (slot_w + gap) + (slot_w - info.disp_w) / 2.0;
            let ty = mb + match cfg.sheet.valign {
                VAlign::Bottom => 0.0,
                VAlign::Center => (inner_h - info.disp_h) / 2.0,
                VAlign::Top => inner_h - info.disp_h,
            };
            let m = place_matrix(&info.bleed, info.rotate, tx, ty);
            let _ = writeln!(
                content,
                "q {} {} {} {} {} {} cm /{} Do Q",
                fmt(m[0]), fmt(m[1]), fmt(m[2]), fmt(m[3]), fmt(m[4]), fmt(m[5]), name
            );
            if first.is_none() {
                first = Some((transform_rect(&m, &info.bleed), transform_rect(&m, &info.trim)));
            }
        }

        if cfg.mark.enabled && mark_w > 0.0 && mark_h > 0.0 {
            if let Some((bleed, trim)) = first {
                let top = match cfg.mark.anchor {
                    MarkAnchor::TrimTop => trim.y1,
                    MarkAnchor::BleedTop => bleed.y1,
                } - cfg.pt(cfg.mark.offset_y);
                let x = bleed.x0 - mark_dx - mark_w;
                let y = top - mark_h;
                if x < -EPS {
                    log::warn!("eye mark clipped by sheet edge: {:.4}{} of {:.4} off the left", -x / cfg.pt(1.0), unit_label(cfg), mark_w / cfg.pt(1.0));
                }
                // 1. White underlay: mark width; sheet top -> sheet bottom (or trim -> trim).
                if under_cs.is_some() {
                    let (uy, uh) = match under.extent {
                        UnderlayExtent::Page => (0.0, sheet_h),
                        UnderlayExtent::Trim => (trim.y0, trim.h()),
                    };
                    let _ = writeln!(
                        content,
                        "q {}/CSunder cs {} scn {} {} {} {} re f Q",
                        if under.overprint { "/GSop gs " } else { "" },
                        fmt(under.tint / 100.0), fmt(x), fmt(uy), fmt(mark_w), fmt(uh)
                    );
                }
                // 2. Eye mark (process colour), overprinting the underlay.
                let [c, mg, yl, kk] = cfg.mark.cmyk.map(|v| v / 100.0);
                let _ = writeln!(
                    content,
                    "q {}{} {} {} {} k {} {} {} {} re f Q",
                    if cfg.mark.overprint { "/GSop gs " } else { "" },
                    fmt(c), fmt(mg), fmt(yl), fmt(kk), fmt(x), fmt(y), fmt(mark_w), fmt(mark_h)
                );
                // 3. Duplicate in place in the die spot colour, overprinting.
                if spot_cs.is_some() {
                    let _ = writeln!(
                        content,
                        "q {}/CSspot cs {} scn {} {} {} {} re f Q",
                        if spot.overprint { "/GSop gs " } else { "" },
                        fmt(spot.tint / 100.0), fmt(x), fmt(y), fmt(mark_w), fmt(mark_h)
                    );
                }
            }
        }

        let mut cstream = Stream::new(Dictionary::new(), content.into_bytes());
        if cfg.output.compress {
            let _ = cstream.compress();
        }
        let content_id = doc.add_object(cstream);
        let media = Rect::new(0.0, 0.0, this_w, sheet_h);
        let page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => rect_obj(&media),
            "TrimBox" => rect_obj(&media),
            "Resources" => {
                let mut r = dictionary! { "XObject" => xobj_dict };
                if mark_on {
                    let mut cs = Dictionary::new();
                    if let Some(id) = spot_cs { cs.set("CSspot", id); }
                    if let Some(id) = under_cs { cs.set("CSunder", id); }
                    if !cs.is_empty() { r.set("ColorSpace", cs); }
                    r.set("ExtGState", dictionary! { "GSop" => gs_op });
                }
                r
            },
            "Contents" => content_id,
        };
        kids.push(Object::Reference(doc.add_object(page)));
        last_w = this_w;
        last_row = row_w;
        last_h = sheet_h;
    }

    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
    );

    // Point the catalog at the new tree; drop structures tied to the old pages.
    let cat = doc.catalog_mut()?;
    cat.set("Pages", pages_id);
    for key in [
        &b"Outlines"[..], b"StructTreeRoot", b"MarkInfo", b"OpenAction", b"PageLabels",
        b"Dests", b"AcroForm", b"Names", b"Threads", b"PageMode", b"Collection",
    ] {
        cat.remove(key);
    }
    doc.prune_objects();
    doc.renumber_objects();

    Ok(ImposeReport {
        across,
        sheets: sheets.len(),
        pages: k,
        sheet_w_pt: last_w,
        sheet_h_pt: last_h,
        row_w_pt: last_row,
        slot_w_pt: slot_w,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribute_even() {
        assert_eq!(distribute(2, 4, Order::Grouped, Remainder::Fill), vec![0, 0, 1, 1]);
        assert_eq!(distribute(2, 4, Order::Collated, Remainder::Fill), vec![0, 1, 0, 1]);
        assert_eq!(distribute(1, 3, Order::Grouped, Remainder::Fill), vec![0, 0, 0]);
    }

    #[test]
    fn distribute_uneven() {
        assert_eq!(distribute(2, 5, Order::Grouped, Remainder::Fill), vec![0, 0, 0, 1, 1]);
        assert_eq!(distribute(2, 5, Order::Grouped, Remainder::Blank), vec![0, 0, 1, 1]);
        assert_eq!(distribute(3, 4, Order::Collated, Remainder::Fill), vec![0, 1, 2, 0]);
        assert_eq!(distribute(4, 4, Order::Grouped, Remainder::Fill), vec![0, 1, 2, 3]);
    }

    #[test]
    fn fit() {
        // 3" bleed width in 12.375" -> 4 across
        assert_eq!(fit_count(12.375 * 72.0, 216.0, 0.0, 0), 4);
        // minus 0.25" mark -> 12.125" still 4
        assert_eq!(fit_count(12.125 * 72.0, 216.0, 0.0, 0), 4);
        assert_eq!(fit_count(12.125 * 72.0, 216.0, 0.0, 2), 2);
        assert_eq!(fit_count(100.0, 216.0, 0.0, 0), 0);
        // exact fit
        assert_eq!(fit_count(432.0, 216.0, 0.0, 0), 2);
    }

    #[test]
    fn rotation_matrices_land_on_target() {
        let b = Rect::new(10.0, 20.0, 110.0, 220.0); // 100 x 200
        for (rot, w, h) in [(0, 100.0, 200.0), (90, 200.0, 100.0), (180, 100.0, 200.0), (270, 200.0, 100.0)] {
            let m = place_matrix(&b, rot, 5.0, 7.0);
            let r = transform_rect(&m, &b);
            assert!((r.x0 - 5.0).abs() < 1e-9 && (r.y0 - 7.0).abs() < 1e-9, "rot {rot}: {r:?}");
            assert!((r.w() - w).abs() < 1e-9 && (r.h() - h).abs() < 1e-9, "rot {rot}: {r:?}");
        }
        // 90 cw: original top-left corner ends up top-right
        let m = place_matrix(&b, 90, 0.0, 0.0);
        let (x, y) = (b.x0, b.y1);
        assert!(((m[0] * x + m[2] * y + m[4]) - 200.0).abs() < 1e-9);
        assert!(((m[1] * x + m[3] * y + m[5]) - 100.0).abs() < 1e-9);
    }
}

fn unit_label(cfg: &Config) -> &'static str {
    match cfg.units {
        crate::row::config::Units::In => "\"",
        crate::row::config::Units::Mm => "mm",
        crate::row::config::Units::Pt => "pt",
    }
}
