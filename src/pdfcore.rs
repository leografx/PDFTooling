//! Shared PDF plumbing for the imposition tools: geometry, page boxes,
//! page → Form XObject, spot colours, importing pages from other PDFs,
//! rebuilding the page tree and saving.

use anyhow::{anyhow, Context, Result};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use std::path::Path;

pub const EPS: f64 = 0.01;

// ------------------------------------------------------------- geometry --

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn new(a: f64, b: f64, c: f64, d: f64) -> Self {
        Rect { x0: a.min(c), y0: b.min(d), x1: a.max(c), y1: b.max(d) }
    }
    pub fn xywh(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect::new(x, y, x + w, y + h)
    }
    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }
    pub fn cx(&self) -> f64 {
        (self.x0 + self.x1) / 2.0
    }
    pub fn cy(&self) -> f64 {
        (self.y0 + self.y1) / 2.0
    }
    pub fn grow(&self, d: f64) -> Rect {
        Rect { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }
    /// Intersection; returns self unchanged if they don't overlap.
    pub fn intersect(&self, o: &Rect) -> Rect {
        let r = Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) };
        if r.w() <= 0.0 || r.h() <= 0.0 { *self } else { r }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x0 < o.x1 - EPS && o.x0 < self.x1 - EPS && self.y0 < o.y1 - EPS && o.y0 < self.y1 - EPS
    }
    pub fn approx_eq(&self, o: &Rect) -> bool {
        (self.x0 - o.x0).abs() < EPS && (self.y0 - o.y0).abs() < EPS
            && (self.x1 - o.x1).abs() < EPS && (self.y1 - o.y1).abs() < EPS
    }
}

/// 2-D affine matrix in PDF order [a b c d e f]:  x' = a x + c y + e,  y' = b x + d y + f
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat(pub [f64; 6]);

impl Mat {
    pub const I: Mat = Mat([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    pub fn translate(tx: f64, ty: f64) -> Mat {
        Mat([1.0, 0.0, 0.0, 1.0, tx, ty])
    }
    pub fn scale(sx: f64, sy: f64) -> Mat {
        Mat([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }
    /// Counter-clockwise rotation by a multiple of 90° (exact).
    pub fn rot90(quarter_turns_ccw: i32) -> Mat {
        match quarter_turns_ccw.rem_euclid(4) {
            1 => Mat([0.0, 1.0, -1.0, 0.0, 0.0, 0.0]),
            2 => Mat([-1.0, 0.0, 0.0, -1.0, 0.0, 0.0]),
            3 => Mat([0.0, -1.0, 1.0, 0.0, 0.0, 0.0]),
            _ => Mat::I,
        }
    }
    /// Mirror across the vertical line x = c.
    pub fn mirror_x(c: f64) -> Mat {
        Mat([-1.0, 0.0, 0.0, 1.0, 2.0 * c, 0.0])
    }
    /// Mirror across the horizontal line y = c.
    pub fn mirror_y(c: f64) -> Mat {
        Mat([1.0, 0.0, 0.0, -1.0, 0.0, 2.0 * c])
    }
    /// Apply `self` first, then `o`.
    pub fn then(&self, o: &Mat) -> Mat {
        let [a1, b1, c1, d1, e1, f1] = self.0;
        let [a2, b2, c2, d2, e2, f2] = o.0;
        Mat([
            a1 * a2 + b1 * c2,
            a1 * b2 + b1 * d2,
            c1 * a2 + d1 * c2,
            c1 * b2 + d1 * d2,
            e1 * a2 + f1 * c2 + e2,
            e1 * b2 + f1 * d2 + f2,
        ])
    }
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
    pub fn apply_rect(&self, r: &Rect) -> Rect {
        let pts = [(r.x0, r.y0), (r.x1, r.y0), (r.x0, r.y1), (r.x1, r.y1)].map(|(x, y)| self.apply(x, y));
        let mut out = Rect { x0: f64::MAX, y0: f64::MAX, x1: f64::MIN, y1: f64::MIN };
        for (x, y) in pts {
            out.x0 = out.x0.min(x);
            out.x1 = out.x1.max(x);
            out.y0 = out.y0.min(y);
            out.y1 = out.y1.max(y);
        }
        out
    }
    pub fn det(&self) -> f64 {
        self.0[0] * self.0[3] - self.0[1] * self.0[2]
    }
    pub fn linear(&self) -> Mat {
        Mat([self.0[0], self.0[1], self.0[2], self.0[3], 0.0, 0.0])
    }
    pub fn inverse(&self) -> Mat {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
        Mat([ia, ib, ic, id, -(e * ia + f * ic), -(e * ib + f * id)])
    }
    /// Quarter turns (ccw) of a pure rotation, None if mirrored / not axis aligned.
    pub fn quarter_turns(&self) -> Option<i32> {
        (0..4).find(|&q| {
            let r = Mat::rot90(q);
            (0..4).all(|i| (r.0[i] - self.0[i]).abs() < 1e-9)
        })
    }
}

pub fn fmt(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

/// Like `fmt` with 6 decimals: matrix scale/rotation terms need the extra
/// precision (an error of 1e-4 in a scale is 0.03 pt across a letter page).
pub fn fmt6(v: f64) -> String {
    let s = format!("{:.6}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

pub fn cm(m: &Mat) -> String {
    let [a, b, c, d, e, f] = m.0;
    format!("{} {} {} {} {} {} cm", fmt6(a), fmt6(b), fmt6(c), fmt6(d), fmt(e), fmt(f))
}

pub fn rect_obj(r: &Rect) -> Object {
    Object::Array(vec![
        Object::Real(r.x0 as f32),
        Object::Real(r.y0 as f32),
        Object::Real(r.x1 as f32),
        Object::Real(r.y1 as f32),
    ])
}

// ------------------------------------------------------------ page boxes --

pub fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
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

pub fn num(doc: &Document, o: &Object) -> Option<f64> {
    match doc.dereference(o).ok()?.1 {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

pub fn page_box(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Rect> {
    let arr = inherited(doc, page, key)?;
    let a = arr.as_array().ok()?;
    let v: Vec<f64> = a.iter().filter_map(|o| num(doc, o)).collect();
    if v.len() != 4 {
        return None;
    }
    let r = Rect::new(v[0], v[1], v[2], v[3]);
    (r.w() > 0.0 && r.h() > 0.0).then_some(r)
}

/// The boxes of a source page, in unrotated page space.
#[derive(Debug, Clone)]
pub struct PageGeom {
    pub id: ObjectId,
    pub media: Rect,
    pub trim: Rect,
    /// Bleed box from the file (or crop box).
    pub bleed: Rect,
    pub has_bleed_box: bool,
    /// /Rotate, normalised to 0/90/180/270 (clockwise).
    pub rotate: i32,
}

impl PageGeom {
    pub fn read(doc: &Document, id: ObjectId) -> PageGeom {
        let media = page_box(doc, id, b"MediaBox").unwrap_or(Rect::new(0.0, 0.0, 612.0, 792.0));
        let crop = page_box(doc, id, b"CropBox").map(|c| c.intersect(&media)).unwrap_or(media);
        let trim = page_box(doc, id, b"TrimBox").unwrap_or(crop);
        let bleed_box = page_box(doc, id, b"BleedBox");
        let bleed = bleed_box.map(|b| b.intersect(&crop)).unwrap_or(crop);
        let rotate = inherited(doc, id, b"Rotate")
            .and_then(|o| num(doc, &o))
            .map(|r| ((r as i32 % 360) + 360) % 360 / 90 * 90)
            .unwrap_or(0);
        PageGeom { id, media, trim, bleed, has_bleed_box: bleed_box.is_some(), rotate }
    }
    /// Trim size as displayed (after /Rotate).
    pub fn display_size(&self) -> (f64, f64) {
        if self.rotate % 180 == 0 { (self.trim.w(), self.trim.h()) } else { (self.trim.h(), self.trim.w()) }
    }
    /// Bleed available in the file beyond the trim (smallest side).
    pub fn file_bleed(&self) -> f64 {
        [self.trim.x0 - self.bleed.x0, self.trim.y0 - self.bleed.y0, self.bleed.x1 - self.trim.x1, self.bleed.y1 - self.trim.y1]
            .into_iter()
            .fold(f64::MAX, f64::min)
            .max(0.0)
    }
    /// Matrix from unrotated page space to "display space": the page as you
    /// see it, trim box lower-left at the origin.
    pub fn display_matrix(&self) -> Mat {
        let t = &self.trim;
        let (w, h) = (t.w(), t.h());
        // clockwise /Rotate == counter-clockwise turns of (360 - r)
        let to_origin = Mat::translate(-t.x0, -t.y0);
        let rot = match self.rotate {
            90 => Mat::rot90(3).then(&Mat::translate(0.0, w)),
            180 => Mat::rot90(2).then(&Mat::translate(w, h)),
            270 => Mat::rot90(1).then(&Mat::translate(h, 0.0)),
            _ => Mat::I,
        };
        to_origin.then(&rot)
    }
}

// --------------------------------------------------------------- objects --

/// Page → Form XObject clipped to `bbox` (unrotated page space).
pub fn page_to_xobject(doc: &mut Document, page: ObjectId, bbox: &Rect) -> Result<ObjectId> {
    let content = doc.get_page_content(page);
    let resources = match doc.get_dictionary(page).ok().and_then(|d| d.get(b"Resources").ok()) {
        Some(r @ Object::Reference(_)) => r.clone(),
        _ => inherited(doc, page, b"Resources").unwrap_or_else(|| Object::Dictionary(Dictionary::new())),
    };
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "FormType" => 1,
        "BBox" => rect_obj(bbox),
        "Resources" => resources,
    };
    if let Ok(group) = doc.get_dictionary(page).and_then(|d| d.get(b"Group")) {
        dict.set("Group", group.clone());
    }
    let mut stream = Stream::new(dict, content);
    let _ = stream.compress();
    Ok(doc.add_object(stream))
}

/// Separation (spot colour) with a CMYK (percent) alternate.
pub fn add_separation(doc: &mut Document, name: &str, cmyk: [f64; 4]) -> ObjectId {
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

/// Copy page `page_no` (1-based) of another PDF into `doc` as a Form XObject.
/// Returns the XObject and its bounding box (crop box, page space).
pub fn import_page(doc: &mut Document, path: &Path, page_no: u32) -> Result<(ObjectId, Rect)> {
    let mut other = Document::load(path).with_context(|| format!("opening {}", path.display()))?;
    if other.is_encrypted() {
        other.decrypt("").map_err(|e| anyhow!("{} is password protected: {e}", path.display()))?;
    }
    other.renumber_objects_with(doc.max_id + 1);
    let pages = other.get_pages();
    let pid = *pages
        .get(&page_no)
        .ok_or_else(|| anyhow!("{} has no page {page_no}", path.display()))?;
    let geom = PageGeom::read(&other, pid);
    let bbox = page_box(&other, pid, b"CropBox").unwrap_or(geom.media);
    doc.max_id = doc.max_id.max(other.max_id);
    for (id, obj) in other.objects {
        doc.objects.insert(id, obj);
    }
    let xo = page_to_xobject(doc, pid, &bbox)?;
    Ok((xo, bbox))
}

/// Replace the document's page tree with `kids` (already-built page dicts
/// whose Parent is `pages_id`), drop structures tied to the old pages,
/// prune, and save.
pub fn finish_and_save(doc: &mut Document, pages_id: ObjectId, kids: Vec<Object>, out: &Path, object_streams: bool) -> Result<()> {
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
    );
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

    let tmp = out.with_extension("pdf.part");
    {
        use std::io::Write;
        let file = std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        let mut w = std::io::BufWriter::new(file);
        if object_streams {
            let opts = lopdf::SaveOptions::builder().use_object_streams(true).use_xref_streams(true).build();
            doc.save_with_options(&mut w, opts)?;
        } else {
            doc.save_to(&mut w)?;
        }
        w.flush()?;
    }
    std::fs::rename(&tmp, out).with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Escape a string for a PDF literal string.
pub fn pdf_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '(' | ')' | '\\' => { o.push('\\'); o.push(ch); }
            c if c.is_ascii() && !c.is_ascii_control() => o.push(c),
            _ => o.push('?'),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mat_then_and_inverse() {
        let m = Mat::rot90(1).then(&Mat::translate(5.0, 7.0));
        assert_eq!(m.apply(1.0, 0.0), (5.0, 8.0));
        let i = m.then(&m.inverse());
        for (a, b) in i.0.iter().zip(Mat::I.0.iter()) {
            assert!((a - b).abs() < 1e-9);
        }
        assert_eq!(Mat::rot90(3).quarter_turns(), Some(3));
        assert_eq!(Mat::mirror_x(0.0).quarter_turns(), None);
    }

    #[test]
    fn display_matrix_lands_trim_at_origin() {
        let g = PageGeom {
            id: (1, 0),
            media: Rect::new(0.0, 0.0, 300.0, 400.0),
            trim: Rect::new(10.0, 20.0, 110.0, 220.0),
            bleed: Rect::new(0.0, 10.0, 120.0, 230.0),
            has_bleed_box: true,
            rotate: 0,
        };
        for rot in [0, 90, 180, 270] {
            let g = PageGeom { rotate: rot, ..g.clone() };
            let r = g.display_matrix().apply_rect(&g.trim);
            let (w, h) = g.display_size();
            assert!(r.x0.abs() < 1e-9 && r.y0.abs() < 1e-9 && (r.w() - w).abs() < 1e-9 && (r.h() - h).abs() < 1e-9, "{rot}");
        }
        // 90° clockwise: the page's top-left corner ends up top-right
        let g90 = PageGeom { rotate: 90, ..g };
        let (x, y) = g90.display_matrix().apply(10.0, 220.0);
        assert!((x - 200.0).abs() < 1e-9 && (y - 100.0).abs() < 1e-9);
    }
}
