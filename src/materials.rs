//! Study materials: import, indexing, page search and excerpts.
//!
//! Big textbooks are too large to send to any AI whole (a 1,000-page book is over a
//! million tokens), so each PDF is indexed once: page text goes into SQLite's full-text
//! index (searching costs almost no memory), the table of contents and page labels are
//! saved, and the book is split into small chunk files. Jobs then send small materials
//! whole and only the relevant pages of big ones, as a mini-PDF built from those chunks.

use crate::db::Db;
use crate::model::Material;
use lopdf::{Dictionary, Document, Object, ObjectId};
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

pub const LARGE_PAGES: i64 = 60;
pub const LARGE_TEXT_CHARS: usize = 150_000;
const TEXT_PAGE_CHARS: usize = 3000;
const CHUNK: u32 = 8;

static INDEX_LOCK: Mutex<()> = Mutex::new(());

const STOP: &str = "a an the and or of to in on for is are was were be been being it its this that these those with as by at from \
into about how what why when which who whom whose can could should would will shall may might must do does did done not no \
yes i me my you your we our they them their he she his her if then than so such there here also just only very more most \
some any each every all both either neither one two use using used get got make made let lets show tell explain help please \
teach learn understand know want need like okay ok thanks thank question answer page pg section exercise exercises problem problems";

static STOPWORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| STOP.split_whitespace().collect());

pub fn tokens(text: &str) -> Vec<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9]+").unwrap());
    RE.find_iter(&text.to_lowercase())
        .map(|m| m.as_str().to_string())
        .filter(|t| t.len() > 1 && !STOPWORDS.contains(t.as_str()))
        .collect()
}

// ---------- import ----------

const TEXT_EXT: &[&str] = &["txt", "md", "markdown", "csv", "tex", "json", "py", "js", "java", "c", "cpp", "h", "html", "css",
    "rtf", "xml", "yaml", "yml", "sql", "r", "m", "ts", "rs", "go"];

fn image_mime(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

pub fn safe_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if c.is_alphanumeric() || " ._-()".contains(c) { c } else { '_' }).collect();
    n.chars().take(120).collect()
}

pub fn random_hex() -> String {
    use rand::RngExt;
    let mut r = rand::rng();
    (0..16).map(|_| format!("{:02x}", r.random::<u8>())).collect()
}

/// Copy a file into Lumi's data folder and register it. Indexing happens separately.
pub fn add_file(db: &Db, course_id: i64, src: &Path) -> Result<i64, String> {
    let raw = std::fs::read(src).map_err(|e| format!("couldn't read it ({e})"))?;
    let name = safe_name(&src.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "file".into()));
    let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let size = raw.len() as i64;
    let (kind, mime, text): (&str, String, Option<String>) = if ext == "pdf" {
        ("pdf", "application/pdf".into(), None)
    } else if let Some(m) = image_mime(&ext) {
        ("image", m.into(), None)
    } else if ext == "docx" {
        ("text", "text/plain".into(), Some(docx_text(&raw).ok_or("couldn't read this Word file")?))
    } else if TEXT_EXT.contains(&ext.as_str()) || ext.is_empty() {
        ("text", "text/plain".into(), Some(String::from_utf8_lossy(&raw).into_owned()))
    } else {
        return Err("unsupported type (use PDF, Word, text, or images)".into());
    };
    let mut path: Option<String> = None;
    if kind != "text" {
        let p = db.path("files").join(format!("{}.{ext}", random_hex()));
        std::fs::write(&p, &raw).map_err(|e| format!("couldn't save it ({e})"))?;
        path = Some(p.to_string_lossy().into_owned());
    }
    Ok(db.exec(
        "INSERT INTO materials(course_id, filename, kind, mime, path, text, size, pages) VALUES(?,?,?,?,?,?,?,0)",
        &[&course_id, &name, &kind, &mime, &path, &text, &size],
    ))
}

pub fn add_note(db: &Db, course_id: i64, title: &str, text: &str) -> i64 {
    let t = if title.trim().is_empty() { "Notes".to_string() } else { title.trim().chars().take(120).collect() };
    let size = text.len() as i64;
    db.exec(
        "INSERT INTO materials(course_id, filename, kind, mime, text, size) VALUES(?,?,'text','text/plain',?,?)",
        &[&course_id, &t, &text, &size],
    )
}

pub fn delete(db: &Db, mid: i64) {
    if let Some(m) = Material::get(db, mid) {
        if let Some(p) = &m.path {
            let _ = std::fs::remove_file(p);
        }
        let _ = std::fs::remove_dir_all(chunk_dir(db, mid));
    }
    db.exec("DELETE FROM pages WHERE material_id=?", &[&mid]);
    db.exec("DELETE FROM pages_fts WHERE material_id=?", &[&mid]);
    db.exec("DELETE FROM materials WHERE id=?", &[&mid]);
}

fn docx_text(raw: &[u8]) -> Option<String> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(raw)).ok()?;
    let mut xml = String::new();
    zip.by_name("word/document.xml").ok()?.read_to_string(&mut xml).ok()?;
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<w:t[^>]*>([^<]*)</w:t>|</w:p>|<w:tab/>").unwrap());
    let mut out = String::new();
    for c in RE.captures_iter(&xml) {
        match c.get(1) {
            Some(t) => out.push_str(&t.as_str().replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")),
            None if c.get(0).unwrap().as_str() == "<w:tab/>" => out.push('\t'),
            None => out.push('\n'),
        }
    }
    Some(out)
}

// ---------- indexing ----------

pub fn is_large(m: &Material) -> bool {
    match m.kind.as_str() {
        "pdf" => m.pages > LARGE_PAGES,
        "text" => m.text.as_ref().map(|t| t.len() > LARGE_TEXT_CHARS).unwrap_or(false) || m.pages > 0 && m.indexed && m.text.is_none() && m.pages * 3000 > LARGE_TEXT_CHARS as i64,
        _ => false,
    }
}

fn chunk_dir(db: &Db, mid: i64) -> PathBuf {
    db.path("chunks").join(mid.to_string())
}

/// Make sure a material is fully indexed (page text, search index, outline, chunks). Idempotent.
pub fn ensure_ready(db: &Db, mid: i64) {
    let _g = INDEX_LOCK.lock().unwrap();
    let Some(m) = Material::get(db, mid) else { return };
    if !m.indexed {
        index_material(db, &m);
    }
    let Some(m) = Material::get(db, mid) else { return };
    if db.int("SELECT fts FROM materials WHERE id=?", &[&mid]) == 0 {
        fill_fts(db, mid);
    }
    if m.kind == "pdf" && is_large(&m) && !m.chunked {
        if let Some(path) = &m.path {
            if let Ok(doc) = Document::load(path) {
                write_chunks(db, mid, &doc);
            }
        }
    }
}

/// Index everything that isn't yet, one material at a time, in the background.
pub fn ensure_all_background(db: crate::db::Shared) {
    std::thread::spawn(move || {
        let ids: Vec<i64> = db.query("SELECT id FROM materials WHERE kind IN ('pdf','text') AND (indexed=0 OR fts=0 OR (kind='pdf' AND pages>? AND chunked=0))",
            &[&LARGE_PAGES], |r| r.get(0));
        for id in ids {
            ensure_ready(&db, id);
        }
    });
}

fn fill_fts(db: &Db, mid: i64) {
    db.tx(|c| {
        c.execute("DELETE FROM pages_fts WHERE material_id=?", [mid])?;
        c.execute("INSERT INTO pages_fts(text, material_id, page) SELECT text, material_id, page FROM pages WHERE material_id=?", [mid])?;
        c.execute("UPDATE materials SET fts=1 WHERE id=?", [mid])?;
        Ok(())
    });
}

fn store_pages(db: &Db, mid: i64, pages: impl Iterator<Item = (u32, String)>) {
    db.tx(|c| {
        c.execute("DELETE FROM pages WHERE material_id=?", [mid])?;
        let mut st = c.prepare("INSERT INTO pages(material_id, page, text) VALUES(?,?,?)")?;
        for (p, t) in pages {
            st.execute(rusqlite::params![mid, p, t])?;
        }
        Ok(())
    });
}

fn index_material(db: &Db, m: &Material) {
    match m.kind.as_str() {
        "pdf" => {
            let Some(path) = &m.path else { return };
            let doc = match Document::load(path) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("[lumi] couldn't open {}: {e}", m.filename);
                    db.exec("UPDATE materials SET indexed=1 WHERE id=?", &[&m.id]);
                    return;
                }
            };
            let n = doc.get_pages().len() as u32;
            let texts = (1..=n).map(|p| {
                let mut s = String::new();
                // pdf-extract can panic on unusual fonts; one bad page shouldn't stop the book
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut out = pdf_extract::PlainTextOutput::new(&mut s);
                    let _ = pdf_extract::output_doc_page(&doc, &mut out, p);
                }));
                if r.is_err() {
                    s.clear();
                }
                (p, s)
            });
            store_pages(db, m.id, texts);
            let outline = toc(&doc);
            let labels = page_labels(&doc, n);
            let n = n as i64;
            db.exec("UPDATE materials SET pages=?, outline=?, page_labels=?, indexed=1 WHERE id=?",
                &[&n, &serde_json::to_string(&outline).unwrap(), &serde_json::to_string(&labels).unwrap(), &m.id]);
            if n > LARGE_PAGES {
                write_chunks(db, m.id, &doc);
            }
        }
        "text" => {
            let t = m.text.clone().unwrap_or_default();
            let chars: Vec<char> = t.chars().collect();
            let pages: Vec<(u32, String)> = chars.chunks(TEXT_PAGE_CHARS).enumerate().map(|(i, c)| (i as u32 + 1, c.iter().collect())).collect();
            let n = pages.len().max(1) as i64;
            store_pages(db, m.id, pages.into_iter());
            db.exec("UPDATE materials SET pages=?, indexed=1 WHERE id=?", &[&n, &m.id]);
        }
        _ => {
            db.exec("UPDATE materials SET indexed=1 WHERE id=?", &[&m.id]);
        }
    }
}

/// [[depth, title, page], ...] with depth starting at 0 (same format as earlier versions).
fn toc(doc: &Document) -> Vec<(i64, String, i64)> {
    match doc.get_toc() {
        Ok(t) => t.toc.iter().map(|e| (e.level as i64 - 1, e.title.trim().to_string(), e.page as i64)).collect(),
        Err(_) => vec![],
    }
}

/// Printed page labels ("i", "ii", "1", "2", ...) from the PDF's /PageLabels.
fn page_labels(doc: &Document, n: u32) -> Vec<String> {
    let mut ranges: Vec<(u32, Dictionary)> = vec![];
    fn collect(doc: &Document, node: &Dictionary, out: &mut Vec<(u32, Dictionary)>) {
        if let Ok(Object::Array(nums)) = node.get(b"Nums").map(|o| deref(doc, o)) {
            for pair in nums.chunks(2) {
                if let [Object::Integer(i), d] = pair {
                    if let Object::Dictionary(d) = deref(doc, d) {
                        out.push((*i as u32, d));
                    }
                }
            }
        }
        if let Ok(Object::Array(kids)) = node.get(b"Kids").map(|o| deref(doc, o)) {
            for k in kids {
                if let Object::Dictionary(d) = deref(doc, &k) {
                    collect(doc, &d, out);
                }
            }
        }
    }
    if let Ok(cat) = doc.catalog() {
        if let Ok(o) = cat.get(b"PageLabels") {
            if let Object::Dictionary(d) = deref(doc, o) {
                collect(doc, &d, &mut ranges);
            }
        }
    }
    if ranges.is_empty() {
        return vec![];
    }
    ranges.sort_by_key(|r| r.0);
    (0..n)
        .map(|i| {
            let (start, d) = ranges.iter().rev().find(|r| r.0 <= i).unwrap_or(&ranges[0]);
            let st = d.get(b"St").ok().and_then(|o| o.as_i64().ok()).unwrap_or(1);
            let num = st + (i - start) as i64;
            let prefix = d.get(b"P").ok().and_then(|o| o.as_str().ok()).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
            let style = d.get(b"S").ok().and_then(|o| o.as_name().ok()).map(|b| b.to_vec());
            let body = match style.as_deref() {
                Some(b"D") => num.to_string(),
                Some(b"r") => roman(num).to_lowercase(),
                Some(b"R") => roman(num),
                Some(b"a") => alpha(num).to_lowercase(),
                Some(b"A") => alpha(num),
                _ => String::new(),
            };
            prefix + &body
        })
        .collect()
}

fn deref(doc: &Document, o: &Object) -> Object {
    match o {
        Object::Reference(id) => doc.get_object(*id).cloned().unwrap_or(Object::Null),
        x => x.clone(),
    }
}

fn roman(mut n: i64) -> String {
    let t = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut s = String::new();
    for (v, r) in t {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

fn alpha(n: i64) -> String {
    let c = (b'A' + ((n - 1) % 26) as u8) as char;
    std::iter::repeat_n(c, ((n - 1) / 26 + 1) as usize).collect()
}

// ---------- chunks & excerpts ----------

fn write_chunks(db: &Db, mid: i64, doc: &Document) {
    let dir = chunk_dir(db, mid);
    let _ = std::fs::create_dir_all(&dir);
    let n = doc.get_pages().len() as u32;
    let mut start = 1;
    while start <= n {
        let pages: Vec<u32> = (start..=(start + CHUNK - 1).min(n)).collect();
        if let Some(bytes) = copy_pages(&[(doc, pages)]) {
            let _ = std::fs::write(dir.join(format!("{start}.pdf")), bytes);
        }
        start += CHUNK;
    }
    db.exec("UPDATE materials SET chunked=1 WHERE id=?", &[&mid]);
}

/// A small PDF of just these pages (cached on disk). Reads only the chunk files it needs.
pub fn excerpt_pdf(db: &Db, m: &Material, pages: &[u32]) -> Option<Vec<u8>> {
    let key = format!("{}-{}", m.id, pages.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("_"));
    let key = if key.len() > 120 { format!("{}-{:x}", m.id, fxhash(&key)) } else { key };
    let path = db.path("excerpts").join(format!("{key}.pdf"));
    if let Ok(b) = std::fs::read(&path) {
        return Some(b);
    }
    let bytes = if m.chunked {
        let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &p in pages {
            let start = (p - 1) / CHUNK * CHUNK + 1;
            groups.entry(start).or_default().push(p - start + 1);
        }
        let docs: Vec<(Document, Vec<u32>)> = groups
            .into_iter()
            .filter_map(|(start, ps)| Document::load(chunk_dir(db, m.id).join(format!("{start}.pdf"))).ok().map(|d| (d, ps)))
            .collect();
        let refs: Vec<(&Document, Vec<u32>)> = docs.iter().map(|(d, p)| (d, p.clone())).collect();
        copy_pages(&refs)?
    } else {
        let doc = Document::load(m.path.as_ref()?).ok()?;
        copy_pages(&[(&doc, pages.to_vec())])?
    };
    let _ = std::fs::write(&path, &bytes);
    Some(bytes)
}

fn fxhash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Build a new PDF containing the given pages (1-based) of the given documents.
fn copy_pages(srcs: &[(&Document, Vec<u32>)]) -> Option<Vec<u8>> {
    let mut out = Document::with_version("1.5");
    let pages_id = out.new_object_id();
    let mut kids = vec![];
    for (src, nums) in srcs {
        let page_map = src.get_pages();
        let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();
        for n in nums {
            let Some(&pid) = page_map.get(n) else { continue };
            let Ok(dict) = src.get_dictionary(pid) else { continue };
            let mut dict = dict.clone();
            for key in [&b"Resources"[..], b"MediaBox", b"CropBox", b"Rotate"] {
                if !dict.has(key) {
                    if let Some(v) = inherited(src, pid, key) {
                        dict.set(key.to_vec(), v);
                    }
                }
            }
            for key in [&b"Parent"[..], b"Annots", b"StructParents", b"B", b"Thumb"] {
                dict.remove(key);
            }
            let mut new_dict = match copy_obj(src, &Object::Dictionary(dict), &mut out, &mut map) {
                Object::Dictionary(d) => d,
                _ => continue,
            };
            new_dict.set("Parent", Object::Reference(pages_id));
            kids.push(Object::Reference(out.add_object(Object::Dictionary(new_dict))));
        }
    }
    if kids.is_empty() {
        return None;
    }
    let count = kids.len() as i64;
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Kids", Object::Array(kids));
    pages.set("Count", Object::Integer(count));
    out.objects.insert(pages_id, Object::Dictionary(pages));
    let mut cat = Dictionary::new();
    cat.set("Type", Object::Name(b"Catalog".to_vec()));
    cat.set("Pages", Object::Reference(pages_id));
    let cat_id = out.add_object(Object::Dictionary(cat));
    out.trailer.set("Root", Object::Reference(cat_id));
    let mut buf = Vec::new();
    out.save_to(&mut buf).ok()?;
    Some(buf)
}

fn inherited(doc: &Document, mut id: ObjectId, key: &[u8]) -> Option<Object> {
    for _ in 0..32 {
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(v) = d.get(key) {
            return Some(v.clone());
        }
        id = d.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn copy_obj(src: &Document, obj: &Object, out: &mut Document, map: &mut HashMap<ObjectId, ObjectId>) -> Object {
    match obj {
        Object::Reference(id) => {
            if let Some(n) = map.get(id) {
                return Object::Reference(*n);
            }
            let n = out.new_object_id();
            map.insert(*id, n);
            let o = src.get_object(*id).cloned().unwrap_or(Object::Null);
            let c = copy_obj(src, &o, out, map);
            out.objects.insert(n, c);
            Object::Reference(n)
        }
        Object::Dictionary(d) => Object::Dictionary(copy_dict(src, d, out, map)),
        Object::Array(a) => Object::Array(a.iter().map(|x| copy_obj(src, x, out, map)).collect()),
        Object::Stream(s) => {
            let mut s2 = s.clone();
            s2.dict = copy_dict(src, &s.dict, out, map);
            Object::Stream(s2)
        }
        x => x.clone(),
    }
}

fn copy_dict(src: &Document, d: &Dictionary, out: &mut Document, map: &mut HashMap<ObjectId, ObjectId>) -> Dictionary {
    let mut nd = Dictionary::new();
    for (k, v) in d.iter() {
        if k == b"Parent" {
            continue;
        }
        nd.set(k.clone(), copy_obj(src, v, out, map));
    }
    nd
}

// ---------- table of contents & retrieval ----------

pub fn outline(m: &Material) -> Vec<(i64, String, i64)> {
    m.outline.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
}

pub fn toc_text(m: &Material) -> String {
    outline(m)
        .iter()
        .filter(|(d, _, _)| *d <= 2)
        .map(|(d, t, p)| format!("{}{t} (PDF page {p})", "  ".repeat(*d as usize)))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn section_of(m: &Material, page: u32) -> Option<String> {
    let mut best: Option<(i64, &String)> = None;
    let o = outline(m);
    for (_, t, p) in &o {
        if *p <= page as i64 && best.map(|b| *p >= b.0).unwrap_or(true) {
            best = Some((*p, t));
        }
    }
    best.map(|b| b.1.clone())
}

fn section_ranges(m: &Material) -> Vec<(String, u32, u32)> {
    let items = outline(m);
    let n = m.pages.max(1) as u32;
    items
        .iter()
        .enumerate()
        .map(|(i, (d, t, p))| {
            let end = items[i + 1..].iter().find(|(d2, _, p2)| d2 <= d && p2 > p).map(|x| x.2 as u32 - 1).unwrap_or(n);
            (t.clone(), *p as u32, end.max(*p as u32))
        })
        .collect()
}

fn skip_pages(m: &Material) -> HashSet<u32> {
    static FRONT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*(cover|title|copyright|contents|table of contents|preface|index|dedication)\b").unwrap());
    let mut s = HashSet::new();
    for (t, a, b) in section_ranges(m) {
        if FRONT.is_match(&t) {
            s.extend(a..=b);
        }
    }
    s
}

/// Pages the learner pointed at: "page 227", "p. 41", "section 4.7", "exercise 4.7 #25".
/// With several sections named (e.g. a test's topic list), each gets an equal share.
fn explicit_pages(m: &Material, query: &str, max_pages: usize) -> Vec<u32> {
    static PAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:page|pg|p\.)\s*(\d{1,4})\b").unwrap());
    static SEC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d{1,2}\.\d{1,2})\b").unwrap());
    static EXER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)exercis|problem|homework|#").unwrap());
    let labels: Vec<String> = m.page_labels.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
    let mut pages = vec![];
    for c in PAGE.captures_iter(query) {
        let lab = &c[1];
        if let Some(i) = labels.iter().position(|l| l == lab) {
            pages.extend([i as u32 + 1, i as u32 + 2]);
        } else if let Ok(p) = lab.parse::<u32>() {
            if p as i64 <= m.pages {
                pages.push(p);
            }
        }
    }
    let exercises = EXER.is_match(query);
    let ranges = section_ranges(m);
    let mut groups: Vec<Vec<u32>> = vec![];
    let mut seen = HashSet::new();
    for c in SEC.captures_iter(query) {
        let num = c[1].to_string();
        if !seen.insert(num.clone()) {
            continue;
        }
        let re = Regex::new(&format!(r"(?i)^\s*(section\s*)?{}\b", regex::escape(&num))).unwrap();
        if let Some((_, a, b)) = ranges.iter().find(|(t, _, _)| re.is_match(t)) {
            let span: Vec<u32> = (*a..=*b).collect();
            groups.push(if exercises { span[span.len().saturating_sub(6)..].to_vec() } else { span });
        }
    }
    if !groups.is_empty() {
        let share = ((max_pages.saturating_sub(pages.len())) / groups.len()).max(2);
        for g in groups {
            pages.extend(g.into_iter().take(share));
        }
    }
    pages
}

fn fts_scores(db: &Db, mid: i64, query: &str) -> Vec<(u32, f64)> {
    let toks: Vec<String> = tokens(query).into_iter().filter(|t| !t.chars().all(|c| c.is_ascii_digit())).collect();
    if toks.is_empty() {
        return vec![];
    }
    let uniq: Vec<String> = toks.iter().cloned().collect::<std::collections::BTreeSet<_>>().into_iter().take(60).collect();
    let q = uniq.iter().map(|t| format!("\"{t}\"")).collect::<Vec<_>>().join(" OR ");
    db.query(
        "SELECT page, -bm25(pages_fts) FROM pages_fts WHERE pages_fts MATCH ? AND material_id=? ORDER BY rank LIMIT 60",
        &[&q, &mid],
        |r| Ok((r.get::<_, i64>(0)? as u32, r.get::<_, f64>(1)?)),
    )
}

/// The most relevant pages for the query, in reading order.
pub fn retrieve(db: &Db, m: &Material, query: &str, max_pages: usize, exclude: &HashSet<u32>) -> Vec<u32> {
    let mut chosen: Vec<u32> = vec![];
    let push = |p: u32, chosen: &mut Vec<u32>| {
        if !chosen.contains(&p) && !exclude.contains(&p) && p >= 1 && (p as i64) <= m.pages.max(1) && chosen.len() < max_pages {
            chosen.push(p);
        }
    };
    for p in explicit_pages(m, query, max_pages) {
        push(p, &mut chosen);
    }
    static REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:page|pg|p\.)\s*\d{1,4}\b|\b\d{1,2}\.\d{1,2}\b").unwrap());
    let rest = REF.replace_all(query, " ").to_string();
    if tokens(&rest).len() >= 2 {
        let skip = skip_pages(m);
        let scores: Vec<(u32, f64)> = fts_scores(db, m.id, &rest).into_iter().filter(|(p, _)| !skip.contains(p)).collect();
        if let Some(top) = scores.first().map(|s| s.1) {
            // the best-matching section by title gets its opening pages (definitions live there)
            let q: HashSet<String> = tokens(&rest).into_iter().collect();
            let best = section_ranges(m)
                .into_iter()
                .map(|(t, a, b)| (tokens(&t).into_iter().collect::<HashSet<_>>().intersection(&q).count(), a, b))
                .max_by_key(|x| x.0);
            if let Some((overlap, a, b)) = best {
                if overlap >= 2 {
                    for p in a..=b.min(a + 4) {
                        push(p, &mut chosen);
                    }
                }
            }
            let scored: HashMap<u32, f64> = scores.iter().cloned().collect();
            for (p, s) in &scores {
                if chosen.len() >= max_pages || *s < 0.45 * top {
                    break;
                }
                push(*p, &mut chosen);
                if scored.contains_key(&(p + 1)) {
                    push(p + 1, &mut chosen); // content tends to continue onto the next page
                }
            }
        }
    }
    chosen.sort();
    chosen
}

pub fn page_texts(db: &Db, m: &Material, pages: &[u32]) -> String {
    pages
        .iter()
        .map(|p| {
            let t: String = db.one("SELECT text FROM pages WHERE material_id=? AND page=?", &[&m.id, p], |r| r.get::<_, Option<String>>(0)).flatten().unwrap_or_default();
            let sec = section_of(m, *p).map(|s| format!(" ({s})")).unwrap_or_default();
            format!("--- {}, PDF page {p}{sec} ---\n{t}", m.filename)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn whole_text(db: &Db, m: &Material) -> String {
    if m.kind == "text" {
        if let Some(t) = &m.text {
            return t.clone();
        }
    }
    db.query("SELECT page, text FROM pages WHERE material_id=? ORDER BY page", &[&m.id], |r| {
        Ok(format!("--- page {} ---\n{}", r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default()))
    })
    .join("\n")
}

/// "PDF pages 251-255 (4.7 Indirect Argument ...)".
pub fn label_pages(m: &Material, pages: &[u32]) -> String {
    if pages.is_empty() {
        return String::new();
    }
    let mut spans = vec![];
    let (mut start, mut prev) = (pages[0], pages[0]);
    for &p in &pages[1..] {
        if p == prev + 1 {
            prev = p;
            continue;
        }
        spans.push(if prev != start { format!("{start}-{prev}") } else { start.to_string() });
        start = p;
        prev = p;
    }
    spans.push(if prev != start { format!("{start}-{prev}") } else { start.to_string() });
    let mut secs: Vec<String> = vec![];
    for p in pages {
        if let Some(s) = section_of(m, *p) {
            if !secs.contains(&s) {
                secs.push(s);
            }
        }
    }
    let secs = if secs.is_empty() { String::new() } else { format!(" ({})", secs.into_iter().take(3).collect::<Vec<_>>().join("; ")) };
    format!("PDF pages {}{secs}", spans.join(", "))
}
