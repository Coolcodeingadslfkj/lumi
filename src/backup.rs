//! Backups: everything in one .zip (minus API keys and caches), and restoring one on the next start.
//!
//! A restore can't swap the database while it's open, so `stage_restore` unpacks the backup into
//! `restore-pending/` and `finish_pending_restore` (called before the database opens) moves it into place.

use crate::db::Db;
use rusqlite::Connection;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const KEY_SETTINGS: [&str; 5] = ["gemini_key", "groq_key", "mistral_key", "openrouter_key", "api_key"];
/// Folders worth keeping. `chunks` and `excerpts` are caches that rebuild themselves.
const FOLDERS: [&str; 2] = ["files", "chat_images"];
const MANIFEST: &str = "lumi-backup.json";

pub fn default_name() -> String {
    format!("lumi-backup-{}.zip", chrono::Local::now().format("%Y-%m-%d"))
}

/// Write a backup zip to `dest`. API keys are left out so the file is safe to keep anywhere.
pub fn export(db: &Db, dest: &Path) -> Result<(), String> {
    let snap = db.path("backup-snapshot.db");
    let _ = fs::remove_file(&snap);
    // a consistent copy of the live database, then scrub keys (VACUUM so they don't linger in free pages)
    db.exec_checked("VACUUM INTO ?", &[&snap.to_string_lossy().to_string()]).map_err(|e| format!("couldn't copy the database ({e})"))?;
    let result = (|| {
        let c = Connection::open(&snap).map_err(|e| e.to_string())?;
        for k in KEY_SETTINGS {
            c.execute("DELETE FROM settings WHERE key=?", [k]).map_err(|e| e.to_string())?;
        }
        c.execute_batch("VACUUM").map_err(|e| e.to_string())?;
        drop(c);
        let file = fs::File::create(dest).map_err(|e| format!("couldn't create {} ({e})", dest.display()))?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let manifest = serde_json::json!({"app": "lumi", "version": env!("CARGO_PKG_VERSION"), "created": chrono::Local::now().to_rfc3339()});
        zip.start_file(MANIFEST, opts).map_err(|e| e.to_string())?;
        zip.write_all(manifest.to_string().as_bytes()).map_err(|e| e.to_string())?;
        add_file(&mut zip, &snap, "lumi.db", opts)?;
        for folder in FOLDERS {
            let Ok(entries) = fs::read_dir(db.path(folder)) else { continue };
            for e in entries.flatten() {
                if e.path().is_file() {
                    let name = format!("{folder}/{}", e.file_name().to_string_lossy());
                    add_file(&mut zip, &e.path(), &name, opts)?;
                }
            }
        }
        zip.finish().map_err(|e| e.to_string())?;
        Ok(())
    })();
    let _ = fs::remove_file(&snap);
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

fn add_file(zip: &mut zip::ZipWriter<fs::File>, path: &Path, name: &str, opts: zip::write::SimpleFileOptions) -> Result<(), String> {
    zip.start_file(name, opts).map_err(|e| e.to_string())?;
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    std::io::copy(&mut f, zip).map_err(|e| e.to_string())?;
    Ok(())
}

/// Check and unpack a backup so it replaces the current data the next time Lumi starts.
pub fn stage_restore(data_dir: &Path, zip_path: &Path) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "That isn't a Lumi backup (not a zip file).".to_string())?;
    if zip.by_name(MANIFEST).is_err() || zip.by_name("lumi.db").is_err() {
        return Err("That zip isn't a Lumi backup.".into());
    }
    let pending = data_dir.join("restore-pending");
    let _ = fs::remove_dir_all(&pending);
    fs::create_dir_all(&pending).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        // only known names; never write outside the pending folder
        let Some(rel) = entry.enclosed_name() else { continue };
        let top = rel.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
        if !(rel == Path::new("lumi.db") || rel == Path::new(MANIFEST) || FOLDERS.contains(&top.as_str())) || entry.is_dir() {
            continue;
        }
        let out = pending.join(&rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        fs::write(&out, buf).map_err(|e| e.to_string())?;
    }
    // make sure it's really a Lumi database before promising anything
    let ok = Connection::open(pending.join("lumi.db"))
        .and_then(|c| c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('courses', 'settings')", [], |r| r.get::<_, i64>(0)))
        .map(|n| n == 2)
        .unwrap_or(false);
    if !ok {
        let _ = fs::remove_dir_all(&pending);
        return Err("The database in that backup is damaged or isn't from Lumi.".into());
    }
    Ok(())
}

/// If a restore is waiting, swap it in. The old data is kept in `before-restore/`; API keys carry over.
pub fn finish_pending_restore(data_dir: &Path) -> Result<bool, String> {
    let pending = data_dir.join("restore-pending");
    if !pending.join("lumi.db").exists() {
        return Ok(false);
    }
    let live = data_dir.join("lumi.db");
    let keys: Vec<(String, String)> = Connection::open(&live)
        .ok()
        .map(|c| {
            KEY_SETTINGS.iter().filter_map(|k| c.query_row("SELECT value FROM settings WHERE key=?", [k], |r| r.get::<_, String>(0)).ok().map(|v| (k.to_string(), v))).collect()
        })
        .unwrap_or_default();
    let old = data_dir.join("before-restore");
    let _ = fs::remove_dir_all(&old);
    fs::create_dir_all(&old).map_err(|e| e.to_string())?;
    for name in ["lumi.db", "lumi.db-wal", "lumi.db-shm", "files", "chat_images"] {
        let p = data_dir.join(name);
        if p.exists() {
            fs::rename(&p, old.join(name)).map_err(|e| format!("couldn't move {name} aside ({e})"))?;
        }
    }
    for name in ["lumi.db", "files", "chat_images"] {
        let p = pending.join(name);
        if p.exists() {
            fs::rename(&p, data_dir.join(name)).map_err(|e| format!("couldn't move {name} into place ({e})"))?;
        }
    }
    // caches belong to the old data
    for cache in ["chunks", "excerpts"] {
        let _ = fs::remove_dir_all(data_dir.join(cache));
    }
    let c = Connection::open(&live).map_err(|e| e.to_string())?;
    let files: PathBuf = data_dir.join("files");
    // file paths were absolute on the computer the backup came from
    let mut stmt = c.prepare("SELECT id, path FROM materials WHERE path IS NOT NULL").map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    drop(stmt);
    for (id, path) in rows {
        let name = Path::new(&path.replace('\\', "/")).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        c.execute("UPDATE materials SET path=? WHERE id=?", rusqlite::params![files.join(name).to_string_lossy().to_string(), id]).map_err(|e| e.to_string())?;
    }
    let has_chunked = c.prepare("SELECT chunked FROM materials LIMIT 0").is_ok();
    if has_chunked {
        c.execute("UPDATE materials SET chunked=0", []).map_err(|e| e.to_string())?;
    }
    for (k, v) in keys {
        c.execute("INSERT OR IGNORE INTO settings(key, value) VALUES(?,?)", [k, v]).map_err(|e| e.to_string())?;
    }
    drop(c);
    let _ = fs::remove_dir_all(&pending);
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn backup_round_trip_drops_keys_and_fixes_paths() {
        let base = std::env::temp_dir().join(format!("lumi-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, b) = (base.join("a"), base.join("b"));
        let db = crate::db::Db::open_at(&a).unwrap();
        db.set("gemini_key", &"secret-a");
        db.set("name", &"Sam");
        let cid = crate::actions::new_course(&db, "Bio", "", "🧬");
        let f = db.path("files").join("abc.pdf");
        std::fs::write(&f, b"%PDF-1.4 fake").unwrap();
        db.exec("INSERT INTO materials(course_id, filename, kind, path) VALUES(?,?,?,?)", &[&cid, &"x.pdf", &"pdf", &f.to_string_lossy().to_string()]);
        let zip = base.join("backup.zip");
        super::export(&db, &zip).unwrap();
        let raw = std::fs::read(&zip).unwrap();
        assert!(!raw.windows(8).any(|w| w == b"secret-a"), "keys must not be in the backup");

        // restore onto another "computer" that has its own key
        let other = crate::db::Db::open_at(&b).unwrap();
        other.set("groq_key", &"secret-b");
        drop(other);
        super::stage_restore(&b, &zip).unwrap();
        assert!(super::finish_pending_restore(&b).unwrap());
        let db2 = crate::db::Db::open_at(&b).unwrap();
        assert_eq!(db2.get::<String>("name").as_deref(), Some("Sam"));
        assert_eq!(db2.get::<String>("groq_key").as_deref(), Some("secret-b"), "this computer's keys are kept");
        assert_eq!(db2.get::<String>("gemini_key"), None);
        let path: String = db2.one("SELECT path FROM materials", &[], |r| r.get(0)).unwrap();
        assert!(std::path::Path::new(&path).starts_with(&b) && std::path::Path::new(&path).exists());
        assert!(b.join("before-restore/lumi.db").exists());
    }
}
