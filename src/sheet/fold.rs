//! Signature templates.
//!
//! Folded signatures are worked out by *simulating the folds* on a grid of
//! cells: every fold reflects half the stack onto the other half (reversing
//! its layer order). After the last fold the stack is read like a book —
//! top leaf first, front then back — which gives each cell-face its page
//! number. Each face's orientation on the flat sheet is the inverse of
//! the transform the folds (plus reading/turning) applied to it, so every page
//! comes out upright in the finished section. This covers 4, 8, 16 and 32-page
//! right-angle folds for left, right and top binding without lookup tables.

use crate::sheet::config::{Edge, Flip};
use crate::pdfcore::{Mat, Rect};

/// Saddle-stitch lip (lap): extra paper on the face of one half of the
/// signature so the stitcher's opener can grab it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lip {
    pub side: LipSide,
    pub amount: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LipSide {
    None,
    /// On the half holding the lower page numbers (front half).
    Low,
    /// On the half holding the higher page numbers (back half).
    High,
}

impl Lip {
    #[cfg_attr(not(test), allow(dead_code))]
    pub const NONE: Lip = Lip { side: LipSide::None, amount: 0.0 };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}

#[derive(Debug, Clone)]
pub struct Face {
    /// 0-based page within the unit (signature or flat piece).
    pub local_page: usize,
    pub side: Side,
    /// Trim rectangle in block coordinates (front view).
    pub trim: Rect,
    /// Page display coords (trim at origin) → block front-view coords.
    /// Back faces are mirrored (det < 0): they are seen through the paper.
    pub m: Mat,
    /// Unit vector toward the spine in page display coords (0,0 = none).
    pub spine_dir: (f64, f64),
}

#[derive(Debug, Clone)]
pub struct Template {
    pub label: String,
    /// Pages per unit.
    #[allow(dead_code)]
    pub pages: usize,
    pub w: f64,
    pub h: f64,
    pub faces: Vec<Face>,
}

/// A flat piece: one page, or front + back.
pub fn flat(pw: f64, ph: f64, duplex: bool, flip: Flip) -> Template {
    let mut faces = vec![Face {
        local_page: 0,
        side: Side::Front,
        trim: Rect::new(0.0, 0.0, pw, ph),
        m: Mat::I,
        spine_dir: (0.0, 0.0),
    }];
    if duplex {
        let m = match flip {
            Flip::Tumble => Mat::mirror_y(ph / 2.0),
            _ => Mat::mirror_x(pw / 2.0),
        };
        faces.push(Face { local_page: 1, side: Side::Back, trim: Rect::new(0.0, 0.0, pw, ph), m, spine_dir: (0.0, 0.0) });
    }
    Template { label: if duplex { "flat 2-sided".into() } else { "flat".into() }, pages: faces.len(), w: pw, h: ph, faces }
}

/// Grid shape (cols × rows) for a signature size.
pub fn grid_for(pages: usize) -> Option<(usize, usize)> {
    match pages {
        4 => Some((2, 1)),
        8 => Some((2, 2)),
        16 => Some((4, 2)),
        32 => Some((4, 4)),
        _ => None,
    }
}

/// Fold order: alternate directions, finishing with a vertical fold (the spine).
fn fold_sequence(cols: usize, rows: usize) -> Vec<char> {
    let (mut v, mut h) = (cols.trailing_zeros(), rows.trailing_zeros());
    let mut rev = Vec::new();
    let mut next = 'v';
    while v > 0 || h > 0 {
        if (next == 'v' && v > 0) || h == 0 {
            rev.push('v');
            v -= 1;
            next = 'h';
        } else {
            rev.push('h');
            h -= 1;
            next = 'v';
        }
    }
    rev.reverse();
    rev
}

/// A folded signature of `pages` pages for a page of `pw × ph` (display size).
/// `spine_gap` goes on spine folds, `fold_gap` on head/face folds.
///
/// Every combination of fold directions is simulated; the layout kept is the
/// one with page 1 on the front, the most heads meeting at interior folds
/// (heads-to-heads, the shop standard) and page 1 upright.
pub fn signature(pages: usize, pw: f64, ph: f64, edge: Edge, spine_gap: f64, fold_gap: f64, lip: Lip) -> Template {
    let (cols, rows) = grid_for(pages).expect("unsupported signature size");
    let nfolds = fold_sequence(cols, rows).len();
    let mut best: Option<(i64, Template)> = None;
    for variant in 0..(1u32 << nfolds) {
        let t = build_signature(pages, cols, rows, variant, pw, ph, edge, spine_gap, fold_gap, lip);
        let score = score(&t);
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, t));
        }
    }
    best.unwrap().1
}

fn head_point(f: &Face, ph_top: (f64, f64)) -> (f64, f64) {
    f.m.apply(ph_top.0, ph_top.1)
}

/// Higher is better: heads at interior folds, then page 1 upright.
fn score(t: &Template) -> i64 {
    let page_w_h = {
        let f = &t.faces[0];
        let inv = f.m.inverse().apply_rect(&f.trim);
        (inv.w(), inv.h())
    };
    let interior = |x: f64, y: f64| x > 1e-6 && x < t.w - 1e-6 && y > 1e-6 && y < t.h - 1e-6;
    let heads = t
        .faces
        .iter()
        .filter(|f| {
            let (x, y) = head_point(f, (page_w_h.0 / 2.0, page_w_h.1));
            interior(x, y)
        })
        .count() as i64;
    let p1 = t.faces.iter().find(|f| f.local_page == 0).unwrap();
    let upright = i64::from(p1.side == Side::Front && p1.m.linear().quarter_turns() == Some(0));
    heads * 10 + upright
}

#[allow(clippy::too_many_arguments)]
fn build_signature(pages: usize, cols: usize, rows: usize, variant: u32, pw: f64, ph: f64, edge: Edge, spine_gap: f64, fold_gap: f64, lip: Lip) -> Template {
    let n = cols * rows;
    let cell = |idx: usize| ((idx % cols) as f64, (idx / cols) as f64);

    // ---- simulate the folds in unit-cell coordinates
    let mut f = vec![Mat::I; n];
    let mut z = vec![0i64; n];
    let (mut x0, mut x1, mut y0, mut y1) = (0.0, cols as f64, 0.0, rows as f64);
    let mut crease = (0.0, 'v'); // position and direction of the last fold
    let mut spine_left = false;
    for (k, fold) in fold_sequence(cols, rows).into_iter().enumerate() {
        let move_high = variant & (1 << k) == 0;
        let zmax = *z.iter().max().unwrap();
        let mid = if fold == 'v' { (x0 + x1) / 2.0 } else { (y0 + y1) / 2.0 };
        for idx in 0..n {
            let (i, j) = cell(idx);
            let (cx, cy) = f[idx].apply(i + 0.5, j + 0.5);
            let c = if fold == 'v' { cx } else { cy };
            if (c > mid) == move_high {
                let r = if fold == 'v' { Mat::mirror_x(mid) } else { Mat::mirror_y(mid) };
                f[idx] = f[idx].then(&r);
                z[idx] = 2 * zmax + 1 - z[idx];
            }
        }
        match (fold, move_high) {
            ('v', true) => x1 = mid,
            ('v', false) => x0 = mid,
            ('h', true) => y1 = mid,
            _ => y0 = mid,
        }
        crease = (mid, fold);
        spine_left = fold == 'v' && !move_high;
    }
    debug_assert_eq!(crease.1, 'v');
    // The spine is the last (vertical) crease. G turns it to the binding edge.
    let spine_side_right = !spine_left;
    let g = match (edge, spine_side_right) {
        (Edge::Left, true) | (Edge::Right, false) => Mat::rot90(2),
        (Edge::Left, false) | (Edge::Right, true) => Mat::I,
        (Edge::Top, true) | (Edge::Bottom, false) => Mat::rot90(1),
        (Edge::Top, false) | (Edge::Bottom, true) => Mat::rot90(3),
    };
    // Turning a leaf over the spine mirrors what you see.
    let turn_leaf = match edge {
        Edge::Left | Edge::Right => Mat::mirror_x(0.0),
        Edge::Top | Edge::Bottom => Mat::mirror_y(0.0),
    };
    let bind_dir = match edge {
        Edge::Left => (-1.0, 0.0),
        Edge::Right => (1.0, 0.0),
        Edge::Top => (0.0, 1.0),
        Edge::Bottom => (0.0, -1.0),
    };

    // layer order: highest z is the top leaf
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(z[i]));
    let mut layer = vec![0usize; n];
    for (l, &idx) in order.iter().enumerate() {
        layer[idx] = l;
    }

    // ---- per face: page number and orientation (linear part)
    struct Lin {
        idx: usize,
        side: Side,
        page: usize,
        a: Mat,
        up: bool,
    }
    let mut lins = Vec::with_capacity(2 * n);
    for idx in 0..n {
        let up_side = if f[idx].det() > 0.0 { Side::Front } else { Side::Back };
        for side in [Side::Front, Side::Back] {
            let up = side == up_side;
            let page = 2 * layer[idx] + usize::from(!up);
            let view = f[idx].linear().then(&g).then(if up { &Mat::I } else { &turn_leaf });
            lins.push(Lin { idx, side, page, a: view.inverse(), up });
        }
    }

    // ---- real geometry
    let page_rect = Rect::new(0.0, 0.0, pw, ph);
    let r0 = lins[0].a.apply_rect(&page_rect);
    let (cw, ch) = (r0.w(), r0.h());
    let is_spine = |idx: usize, px: f64, py: f64| {
        let (x, _) = f[idx].apply(px, py);
        (x - crease.0).abs() < 1e-6
    };

    // ---- lip: extra paper beyond the face of every page in the lip half
    // face direction of each cell on the sheet (opposite the spine), and
    // whether the cell's leaf belongs to the lip half
    let mut face_dir = vec![(0i32, 0i32); n];
    let mut in_lip = vec![false; n];
    for l in &lins {
        let sd = if l.up { bind_dir } else { turn_leaf.apply(bind_dir.0, bind_dir.1) };
        let (dx, dy) = l.a.linear().apply(sd.0, sd.1);
        face_dir[l.idx] = (-dx.round() as i32, -dy.round() as i32);
        let low_half = l.page < pages / 2;
        in_lip[l.idx] = lip.amount > 0.0
            && match lip.side {
                LipSide::Low => low_half,
                LipSide::High => !low_half,
                LipSide::None => false,
            };
    }
    let lip_at = |i: usize, j: usize, dir: (i32, i32)| {
        let idx = j * cols + i;
        if in_lip[idx] && face_dir[idx] == dir { lip.amount } else { 0.0 }
    };
    let max_rows = |f: &dyn Fn(usize) -> f64| (0..rows).map(f).fold(0.0, f64::max);
    let max_cols = |f: &dyn Fn(usize) -> f64| (0..cols).map(f).fold(0.0, f64::max);

    let mut col_x = vec![max_rows(&|j| lip_at(0, j, (-1, 0))); cols];
    for k in 1..cols {
        let gap = if is_spine(k - 1, k as f64, 0.5) { spine_gap } else { fold_gap };
        let extra = max_rows(&|j| lip_at(k - 1, j, (1, 0)) + lip_at(k, j, (-1, 0)));
        col_x[k] = col_x[k - 1] + cw + gap + extra;
    }
    let mut row_y = vec![max_cols(&|i| lip_at(i, 0, (0, -1))); rows];
    for k in 1..rows {
        let gap = if is_spine((k - 1) * cols, 0.5, k as f64) { spine_gap } else { fold_gap };
        let extra = max_cols(&|i| lip_at(i, k - 1, (0, 1)) + lip_at(i, k, (0, -1)));
        row_y[k] = row_y[k - 1] + ch + gap + extra;
    }
    let bw = col_x[cols - 1] + cw + max_rows(&|j| lip_at(cols - 1, j, (1, 0)));
    let bh = row_y[rows - 1] + ch + max_cols(&|i| lip_at(i, rows - 1, (0, 1)));

    let mut faces: Vec<Face> = lins
        .iter()
        .map(|l| {
            let (i, j) = (l.idx % cols, l.idx / cols);
            let trim = Rect::xywh(col_x[i], row_y[j], cw, ch);
            let r = l.a.apply_rect(&page_rect);
            let m = l.a.then(&Mat::translate(trim.x0 - r.x0, trim.y0 - r.y0));
            let spine_dir = if l.up { bind_dir } else { turn_leaf.apply(bind_dir.0, bind_dir.1) };
            Face { local_page: l.page, side: l.side, trim, m, spine_dir }
        })
        .collect();

    // Page 1 belongs on the front (outer form): if it came out on the back,
    // look at the sheet from the other side.
    if faces.iter().any(|f| f.local_page == 0 && f.side == Side::Back) {
        let mx = Mat::mirror_x(bw / 2.0);
        for fc in faces.iter_mut() {
            fc.trim = mx.apply_rect(&fc.trim);
            fc.m = fc.m.then(&mx);
            fc.side = if fc.side == Side::Front { Side::Back } else { Side::Front };
        }
    }
    // Present the block with page 1 upright where possible.
    if let Some(first) = faces.iter().find(|f| f.local_page == 0) {
        if first.m.linear().quarter_turns() == Some(2) {
            let r = Mat::rot90(2).then(&Mat::translate(bw, bh));
            for fc in faces.iter_mut() {
                fc.trim = r.apply_rect(&fc.trim);
                fc.m = fc.m.then(&r);
            }
        }
    }
    faces.sort_by_key(|f| (f.side == Side::Back, f.local_page));
    Template { label: format!("{pages}pp signature ({cols}x{rows})"), pages, w: bw, h: bh, faces }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(t: &Template, side: Side, page: usize) -> (f64, f64) {
        let f = t.faces.iter().find(|f| f.side == side && f.local_page == page).unwrap();
        (f.trim.cx(), f.trim.cy())
    }

    fn check_signature(pages: usize, edge: Edge) {
        let t = signature(pages, 100.0, 140.0, edge, 0.0, 0.0, Lip::NONE);
        // every page exactly once
        let mut seen: Vec<usize> = t.faces.iter().map(|f| f.local_page).collect();
        seen.sort();
        assert_eq!(seen, (0..pages).collect::<Vec<_>>(), "{pages}pp");
        // a leaf's two pages (2k, 2k+1) back each other
        for k in 0..pages / 2 {
            let a = t.faces.iter().find(|f| f.local_page == 2 * k).unwrap();
            let b = t.faces.iter().find(|f| f.local_page == 2 * k + 1).unwrap();
            assert_ne!(a.side, b.side);
            assert!(a.trim.approx_eq(&b.trim), "{pages}pp leaf {k}");
        }
        // front faces non-mirrored, back faces mirrored
        for f in &t.faces {
            assert_eq!(f.m.det() > 0.0, f.side == Side::Front);
        }
    }

    #[test]
    fn signatures_are_consistent() {
        for p in [4, 8, 16, 32] {
            for e in [Edge::Left, Edge::Right, Edge::Top] {
                check_signature(p, e);
            }
        }
    }

    #[test]
    fn four_page_folio_left_bound() {
        // classic: front = 4 | 1, back = 2 | 3 (1-based)
        let t = signature(4, 100.0, 140.0, Edge::Left, 0.0, 0.0, Lip::NONE);
        let (x1, _) = pos(&t, Side::Front, 0);
        let (x4, _) = pos(&t, Side::Front, 3);
        assert!(x4 < x1, "page 4 left of page 1 on the front");
        // back view of a left-right turned sheet: page 2 left of 3
        let (x2, _) = pos(&t, Side::Back, 1);
        let (x3, _) = pos(&t, Side::Back, 2);
        // block coords are front view: behind page 1 (right) is page 2
        assert!(x2 > x3);
        assert_eq!(t.w, 200.0);
        // all pages upright (no rotation) for a folio
        for f in &t.faces {
            let lin = f.m.linear();
            assert!(lin.0[3] > 0.0, "head up");
        }
    }

    #[test]
    fn conjugates_across_spine_sum_to_n_plus_1() {
        for p in [4, 8, 16, 32] {
            let t = signature(p, 100.0, 140.0, Edge::Left, 10.0, 30.0, Lip::NONE);
            // with spine_gap 10 and fold_gap 30, neighbours 10 apart share a spine
            let mut pairs = 0;
            for a in &t.faces {
                for b in &t.faces {
                    if a.side == b.side && (b.trim.x0 - a.trim.x1 - 10.0).abs() < 1e-6 && (a.trim.cy() - b.trim.cy()).abs() < 1e-6 {
                        assert_eq!(a.local_page + b.local_page, p - 1, "{p}pp spine pair");
                        pairs += 1;
                    }
                }
            }
            assert_eq!(pairs, p / 2, "{p}pp: every page sits on a spine");
        }
    }

    #[test]
    fn eight_page_has_heads_together() {
        // 8pp: two rows, one row upside-down, heads meeting at the middle fold
        let t = signature(8, 100.0, 140.0, Edge::Left, 0.0, 0.0, Lip::NONE);
        let mut upright = 0;
        let mut inverted = 0;
        for f in t.faces.iter().filter(|f| f.side == Side::Front) {
            if f.m.0[3] > 0.0 { upright += 1 } else { inverted += 1 }
            let head_y = f.m.apply(50.0, 140.0).1; // top-centre of the page
            assert!((head_y - 140.0).abs() < 1e-6, "head at the middle fold, got {head_y}");
        }
        assert_eq!((upright, inverted), (2, 2));
    }

    #[test]
    fn lip_goes_on_the_chosen_half() {
        // 4pp folio, front = 4 | 1: low folio (pages 1,2) is the right half
        let low = signature(4, 100.0, 140.0, Edge::Left, 0.0, 0.0, Lip { side: LipSide::Low, amount: 18.0 });
        assert_eq!(low.w, 218.0);
        let p1 = low.faces.iter().find(|f| f.local_page == 0).unwrap();
        let p4 = low.faces.iter().find(|f| f.local_page == 3).unwrap();
        assert_eq!((p4.trim.x0, p1.trim.x1), (0.0, 200.0), "lip beyond page 1's face");
        let high = signature(4, 100.0, 140.0, Edge::Left, 0.0, 0.0, Lip { side: LipSide::High, amount: 18.0 });
        let p4 = high.faces.iter().find(|f| f.local_page == 3).unwrap();
        assert_eq!(p4.trim.x0, 18.0, "lip beyond page 4's face");
        // 16pp: face folds between two low-folio pages get the lip on both sides
        for p in [8, 16, 32] {
            let base = signature(p, 100.0, 140.0, Edge::Left, 0.0, 10.0, Lip::NONE);
            let t = signature(p, 100.0, 140.0, Edge::Left, 0.0, 10.0, Lip { side: LipSide::Low, amount: 5.0 });
            assert!(t.w > base.w || t.h > base.h, "{p}pp grows");
            // every low-folio page has ≥ 5 of paper (or a 10+10 fold gap) beyond its face
            for f in t.faces.iter().filter(|f| f.local_page < p / 2 && f.side == Side::Front) {
                let inv = f.m.linear();
                let (dx, dy) = inv.apply(-f.spine_dir.0, -f.spine_dir.1);
                let (fx, fy) = (dx.round(), dy.round());
                let room = t.faces.iter().filter(|o| o.side == Side::Front && !std::ptr::eq(*o, f)).map(|o| {
                    if fx > 0.0 && (o.trim.cy() - f.trim.cy()).abs() < 1e-6 && o.trim.x0 >= f.trim.x1 - 1e-6 { o.trim.x0 - f.trim.x1 }
                    else if fx < 0.0 && (o.trim.cy() - f.trim.cy()).abs() < 1e-6 && o.trim.x1 <= f.trim.x0 + 1e-6 { f.trim.x0 - o.trim.x1 }
                    else if fy > 0.0 && (o.trim.cx() - f.trim.cx()).abs() < 1e-6 && o.trim.y0 >= f.trim.y1 - 1e-6 { o.trim.y0 - f.trim.y1 }
                    else if fy < 0.0 && (o.trim.cx() - f.trim.cx()).abs() < 1e-6 && o.trim.y1 <= f.trim.y0 + 1e-6 { f.trim.y0 - o.trim.y1 }
                    else { f64::MAX }
                }).fold(f64::MAX, f64::min);
                let edge_room = if fx > 0.0 { t.w - f.trim.x1 } else if fx < 0.0 { f.trim.x0 } else if fy > 0.0 { t.h - f.trim.y1 } else { f.trim.y0 };
                let r = room.min(edge_room);
                assert!(r >= 5.0 - 1e-6, "{p}pp page {} face room {r}", f.local_page + 1);
            }
        }
    }
}
