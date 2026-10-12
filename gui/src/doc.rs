//! Reading and writing settings in a `toml_edit` document, so comments,
//! blank lines and key order in the user's file are kept on save.

use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

fn split(key: &str) -> (Vec<&str>, &str) {
    let mut parts: Vec<&str> = key.split('.').collect();
    let last = parts.pop().unwrap_or("");
    (parts, last)
}

/// The table that holds `key`'s last segment, if it exists.
fn parent<'a>(doc: &'a DocumentMut, key: &str) -> Option<&'a dyn TableLike> {
    let (path, _) = split(key);
    let mut t: &dyn TableLike = doc.as_table();
    for p in path {
        t = t.get(p)?.as_table_like()?;
    }
    Some(t)
}

pub fn get<'a>(doc: &'a DocumentMut, key: &str) -> Option<&'a Item> {
    let t = parent(doc, key)?;
    let item = t.get(key.rsplit('.').next().unwrap_or(key))?;
    (!item.is_none()).then_some(item)
}

pub fn has(doc: &DocumentMut, key: &str) -> bool {
    get(doc, key).is_some()
}

pub fn get_value<'a>(doc: &'a DocumentMut, key: &str) -> Option<&'a Value> {
    get(doc, key)?.as_value()
}

pub fn get_f64(doc: &DocumentMut, key: &str) -> Option<f64> {
    let v = get_value(doc, key)?;
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
}
pub fn get_i64(doc: &DocumentMut, key: &str) -> Option<i64> {
    let v = get_value(doc, key)?;
    v.as_integer().or_else(|| v.as_float().map(|f| f.round() as i64))
}
pub fn get_bool(doc: &DocumentMut, key: &str) -> Option<bool> {
    get_value(doc, key)?.as_bool()
}
pub fn get_str(doc: &DocumentMut, key: &str) -> Option<String> {
    get_value(doc, key)?.as_str().map(str::to_string)
}
pub fn get_cmyk(doc: &DocumentMut, key: &str) -> Option<[f64; 4]> {
    let a = get_value(doc, key)?.as_array()?;
    let v: Vec<f64> = a.iter().filter_map(|x| x.as_float().or_else(|| x.as_integer().map(|i| i as f64))).collect();
    (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
}

/// Walk to (creating as needed) the table that holds `key`.
fn parent_mut<'a>(doc: &'a mut DocumentMut, key: &str) -> &'a mut dyn TableLike {
    let (path, _) = split(key);
    let mut t: &mut dyn TableLike = doc.as_table_mut();
    for p in path {
        let missing = t.get(p).map(|i| i.as_table_like().is_none()).unwrap_or(true);
        if missing {
            let mut nt = Table::new();
            nt.set_implicit(true); // no empty [header] when it only holds sub-tables
            t.insert(p, Item::Table(nt));
        }
        t = t.get_mut(p).and_then(|i| i.as_table_like_mut()).expect("table just ensured");
    }
    t
}

fn bare_len(v: &Value) -> isize {
    let mut c = v.clone();
    c.decor_mut().clear();
    c.to_string().chars().count() as isize
}

/// Set `key` to `v`, keeping the old value's spacing and trailing comment.
pub fn set_value(doc: &mut DocumentMut, key: &str, mut v: Value) {
    let last = key.rsplit('.').next().unwrap_or(key).to_string();
    let t = parent_mut(doc, key);
    if let Some(old) = t.get_mut(&last).and_then(|i| i.as_value_mut()) {
        let mut decor = old.decor().clone();
        // keep a trailing comment in the same column when the value's width changes
        let old_len = bare_len(old);
        let new_len = bare_len(&v);
        if let Some(sfx) = decor.suffix().and_then(|r| r.as_str()).map(str::to_string) {
            let spaces = sfx.len() - sfx.trim_start_matches(' ').len();
            if sfx[spaces..].starts_with('#') && old_len != new_len {
                let n = (spaces as isize + old_len - new_len).max(1) as usize;
                decor.set_suffix(format!("{}{}", " ".repeat(n), &sfx[spaces..]));
            }
        }
        *v.decor_mut() = decor;
        *old = v;
    } else {
        t.insert(&last, Item::Value(v));
    }
}

/// Remove `key` so the engine uses its default. Empty implicit tables are
/// left alone (they print nothing).
pub fn remove(doc: &mut DocumentMut, key: &str) {
    let last = key.rsplit('.').next().unwrap_or(key).to_string();
    let (path, _) = split(key);
    let mut t: &mut dyn TableLike = doc.as_table_mut();
    for p in path {
        match t.get_mut(p).and_then(|i| i.as_table_like_mut()) {
            Some(n) => t = n,
            None => return,
        }
    }
    t.remove(&last);
}

/// A number as TOML: whole numbers as integers (`19`, not `19.0`), others
/// rounded to 6 decimals so drag noise never reaches the file.
pub fn num(v: f64) -> Value {
    let r = (v * 1e6).round() / 1e6;
    if r.fract() == 0.0 && r.abs() < 1e15 {
        Value::from(r as i64)
    } else {
        Value::from(r)
    }
}

pub fn cmyk(c: [f64; 4]) -> Value {
    let mut a = toml_edit::Array::new();
    for x in c {
        a.push(num(x));
    }
    Value::Array(a)
}

/// Every leaf key in a table, as dotted paths.
#[cfg(test)]
pub fn collect_keys(t: &dyn TableLike, prefix: &str, out: &mut Vec<String>) {
    for (k, item) in t.iter() {
        let full = if prefix.is_empty() { k.to_string() } else { format!("{prefix}.{k}") };
        match item {
            Item::Table(sub) => collect_keys(sub, &full, out),
            Item::None => {}
            _ => out.push(full),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# my job\nunits = \"in\"\n\n[sheet]  # the press sheet\nwidth  = 19      # wide\nheight = 13\n\n[binding]\nstyle = \"saddle\"   # stitched\n";

    #[test]
    fn edits_keep_comments() {
        let mut d: DocumentMut = SAMPLE.parse().unwrap();
        set_value(&mut d, "sheet.width", num(28.0));
        set_value(&mut d, "binding.lip", num(0.375));
        set_value(&mut d, "marks.signature.size", num(9.0));
        remove(&mut d, "sheet.height");
        let s = d.to_string();
        assert!(s.contains("# my job"), "{s}");
        assert!(s.contains("width  = 28      # wide"), "{s}");
        set_value(&mut d, "binding.style", "perfect".into());
        assert!(d.to_string().contains("style = \"perfect\"  # stitched"), "{}", d);
        set_value(&mut d, "binding.style", "saddle".into());
        assert!(s.contains("[sheet]  # the press sheet"), "{s}");
        assert!(s.contains("style = \"saddle\"   # stitched"), "{s}");
        assert!(s.contains("lip = 0.375"), "{s}");
        assert!(!s.contains("height"), "{s}");
        assert!(s.contains("[marks.signature]\nsize = 9"), "{s}");
        assert!(!s.contains("[marks]\n"), "no empty [marks] header: {s}");
        assert_eq!(get_f64(&d, "sheet.width"), Some(28.0));
        assert_eq!(get_f64(&d, "marks.signature.size"), Some(9.0));
        assert!(!has(&d, "sheet.height"));
        let j: pdf_impose::sheet::config::Job = toml::from_str(&s).unwrap();
        assert_eq!(j.binding.lip, 0.375);
    }

    #[test]
    fn numbers_are_tidy() {
        assert_eq!(num(19.0).to_string(), "19");
        assert_eq!(num(0.1 + 0.2).to_string(), "0.3");
        assert_eq!(cmyk([0.0, 100.0, 0.0, 0.0]).to_string(), "[0, 100, 0, 0]");
    }

    #[test]
    fn templates_round_trip_unchanged() {
        for t in [pdf_impose::sheet::config::DEFAULT_JOB_TOML, pdf_impose::row::config::DEFAULT_CONFIG_TOML] {
            let d: DocumentMut = t.parse().unwrap();
            assert_eq!(d.to_string(), t);
        }
    }
}
