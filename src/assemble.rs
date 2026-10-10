//! Assemble a job from several numbered PDFs (name_001.pdf, name_002.pdf …):
//! natural number ordering, gap/duplicate checks, and a page-preserving merge.

use anyhow::{anyhow, Context, Result};
use lopdf::{dictionary, Document, Object, ObjectId};
use std::path::{Path, PathBuf};

/// Split a file stem into (base, number) when it ends in digits.
/// `strict` requires a separator (`_`, `-`, `.` or space) before the digits.
///   "Book_001" → ("Book", 1)   "Book-02" → ("Book", 2)   "page7" → ("page", 7) unless strict
pub fn split_number(stem: &str, strict: bool) -> Option<(String, u64)> {
    let digits_start = stem.char_indices().rev().take_while(|(_, c)| c.is_ascii_digit()).last().map(|(i, _)| i)?;
    if digits_start == 0 {
        return None; // all digits: no base
    }
    let n: u64 = stem[digits_start..].parse().ok()?;
    let head = &stem[..digits_start];
    let sep = head.ends_with(['_', '-', '.', ' ']);
    if strict && !sep {
        return None;
    }
    let base = head.trim_end_matches(['_', '-', '.', ' ']).to_string();
    if base.is_empty() {
        return None;
    }
    Some((base, n))
}

/// Order files by their trailing number (files without one go last, by name).
/// Returns the order plus human-readable warnings (gaps, duplicates).
pub fn order_numbered(files: &[PathBuf]) -> (Vec<PathBuf>, Vec<String>) {
    let mut keyed: Vec<(Option<u64>, String, PathBuf)> = files
        .iter()
        .map(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            (split_number(&stem, false).map(|x| x.1), stem.to_lowercase(), p.clone())
        })
        .collect();
    keyed.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => x.cmp(&y).then(a.1.cmp(&b.1)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.1.cmp(&b.1),
    });
    let mut warnings = Vec::new();
    let nums: Vec<u64> = keyed.iter().filter_map(|k| k.0).collect();
    if let (Some(&first), Some(&last)) = (nums.first(), nums.last()) {
        let mut dups = Vec::new();
        for w in nums.windows(2) {
            if w[0] == w[1] && !dups.contains(&w[0]) {
                dups.push(w[0]);
            }
        }
        if !dups.is_empty() {
            warnings.push(format!("number(s) used twice: {dups:?} — both files are included"));
        }
        let missing: Vec<u64> = (first..=last).filter(|n| !nums.contains(n)).collect();
        if !missing.is_empty() {
            let shown: Vec<String> = missing.iter().take(12).map(|n| n.to_string()).collect();
            warnings.push(format!(
                "missing number(s) {}{} between {first} and {last}",
                shown.join(", "),
                if missing.len() > 12 { " …" } else { "" }
            ));
        }
        if first > 1 {
            warnings.push(format!("numbering starts at {first}"));
        }
    }
    let unnumbered = keyed.iter().filter(|k| k.0.is_none()).count();
    if unnumbered > 0 && !nums.is_empty() {
        warnings.push(format!("{unnumbered} file(s) without a number were added at the end"));
    }
    (keyed.into_iter().map(|k| k.2).collect(), warnings)
}

/// Raw (possibly indirect) inherited attribute of a page, without dereferencing.
fn inherited_raw(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut id = page;
    for _ in 0..64 {
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(v) = dict.get(key) {
            return Some(v.clone());
        }
        id = dict.get(b"Parent").and_then(Object::as_reference).ok()?;
    }
    None
}

/// Join PDFs page by page, in the given order, into `out`. Returns the page count.
pub fn merge_pdfs(files: &[PathBuf], out: &Path) -> Result<usize> {
    let mut merged = Document::with_version("1.5");
    let pages_id = merged.new_object_id();
    let mut kids: Vec<Object> = Vec::new();

    for path in files {
        let mut d = Document::load(path).with_context(|| format!("opening {}", path.display()))?;
        if d.is_encrypted() {
            d.decrypt("").map_err(|e| anyhow!("{} is password protected: {e}", path.display()))?;
        }
        // copy inherited attributes onto each page so it survives a new parent
        let ids: Vec<ObjectId> = d.get_pages().values().copied().collect();
        for &pid in &ids {
            for key in [&b"MediaBox"[..], b"CropBox", b"Resources", b"Rotate"] {
                let has = d.get_dictionary(pid).map(|p| p.has(key)).unwrap_or(true);
                if !has {
                    if let Some(v) = inherited_raw(&d, pid, key) {
                        if let Ok(p) = d.get_dictionary_mut(pid) {
                            p.set(key.to_vec(), v);
                        }
                    }
                }
            }
        }
        d.renumber_objects_with(merged.max_id + 1);
        let ids: Vec<ObjectId> = d.get_pages().values().copied().collect();
        if ids.is_empty() {
            log::warn!("{} has no pages", path.display());
        }
        merged.max_id = merged.max_id.max(d.max_id);
        for (id, obj) in d.objects {
            merged.objects.insert(id, obj);
        }
        for id in ids {
            if let Ok(p) = merged.get_dictionary_mut(id) {
                p.set("Parent", pages_id);
            }
            kids.push(Object::Reference(id));
        }
    }
    let count = kids.len();
    anyhow::ensure!(count > 0, "no pages in the files to join");
    merged.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count as i64 }),
    );
    let cat = merged.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    merged.trailer.set("Root", cat);
    merged.prune_objects();
    merged.renumber_objects();
    let tmp = out.with_extension("pdf.part");
    {
        use std::io::Write;
        let mut w = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        merged.save_to(&mut w)?;
        w.flush()?;
    }
    std::fs::rename(&tmp, out)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(split_number("Book_001", true), Some(("Book".into(), 1)));
        assert_eq!(split_number("Book_01", true), Some(("Book".into(), 1)));
        assert_eq!(split_number("Book_1", true), Some(("Book".into(), 1)));
        assert_eq!(split_number("Book-12", true), Some(("Book".into(), 12)));
        assert_eq!(split_number("My Book 3", true), Some(("My Book".into(), 3)));
        assert_eq!(split_number("page7", true), None);
        assert_eq!(split_number("page7", false), Some(("page".into(), 7)));
        assert_eq!(split_number("2024", false), None);
        assert_eq!(split_number("cover", false), None);
    }

    #[test]
    fn ordering_is_numeric() {
        let files: Vec<PathBuf> = ["b_10.pdf", "b_2.pdf", "b_001.pdf", "notes.pdf", "b_03.pdf"].iter().map(PathBuf::from).collect();
        let (o, w) = order_numbered(&files);
        let names: Vec<&str> = o.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(names, vec!["b_001.pdf", "b_2.pdf", "b_03.pdf", "b_10.pdf", "notes.pdf"]);
        assert!(w.iter().any(|x| x.contains("missing")), "{w:?}");
        assert!(w.iter().any(|x| x.contains("without a number")));
    }
}
