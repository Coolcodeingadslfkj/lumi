//! SQLite storage. The schema matches earlier Lumi versions, so existing data carries over.

use rusqlite::{Connection, OptionalExtension, Row, ToSql};
use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS courses (
    id INTEGER PRIMARY KEY, name TEXT NOT NULL, emoji TEXT, color TEXT,
    goal TEXT, created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS materials (
    id INTEGER PRIMARY KEY, course_id INTEGER NOT NULL, filename TEXT, kind TEXT,
    mime TEXT, path TEXT, text TEXT, file_id TEXT, size INTEGER, pages INTEGER,
    enabled INTEGER DEFAULT 1, created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS concepts (
    id INTEGER PRIMARY KEY, course_id INTEGER NOT NULL, unit_idx INTEGER, unit_title TEXT,
    idx INTEGER, title TEXT, summary TEXT, mastery REAL DEFAULT 0, box INTEGER DEFAULT 0,
    due_at TEXT, attempts INTEGER DEFAULT 0, correct INTEGER DEFAULT 0, last_seen TEXT
);
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY, course_id INTEGER NOT NULL, concept_id INTEGER, title TEXT,
    material_ids TEXT, messages TEXT DEFAULT '[]', created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS activity (
    day TEXT PRIMARY KEY, xp INTEGER DEFAULT 0, answers INTEGER DEFAULT 0,
    correct INTEGER DEFAULT 0, frozen INTEGER DEFAULT 0
);
CREATE TABLE IF NOT EXISTS achievements (key TEXT PRIMARY KEY, unlocked_at TEXT DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS tests (
    id INTEGER PRIMARY KEY, course_id INTEGER NOT NULL, name TEXT, test_date TEXT, topics TEXT,
    material_ids TEXT, overview TEXT, tips TEXT, analyzed INTEGER DEFAULT 0,
    mock_best INTEGER, mock_last INTEGER, created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS test_concepts (
    test_id INTEGER NOT NULL, concept_id INTEGER NOT NULL, idx INTEGER, priority TEXT, why TEXT,
    status TEXT DEFAULT 'todo', lesson_done INTEGER DEFAULT 0, ready_at TEXT, tested_out INTEGER DEFAULT 0,
    PRIMARY KEY (test_id, concept_id)
);
CREATE TABLE IF NOT EXISTS mistakes (
    id INTEGER PRIMARY KEY, course_id INTEGER, concept_id INTEGER, question TEXT, user_answer TEXT,
    times_missed INTEGER DEFAULT 1, fixed INTEGER DEFAULT 0, fixed_at TEXT,
    created_at TEXT DEFAULT (datetime('now')), updated_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS quest_progress (day TEXT, key TEXT, value INTEGER DEFAULT 0, PRIMARY KEY (day, key));
CREATE TABLE IF NOT EXISTS bosses (course_id INTEGER, unit_idx INTEGER, defeated_at TEXT DEFAULT (datetime('now')), PRIMARY KEY (course_id, unit_idx));
CREATE TABLE IF NOT EXISTS pages (material_id INTEGER, page INTEGER, text TEXT, PRIMARY KEY (material_id, page));
CREATE TABLE IF NOT EXISTS quizzes (
    id INTEGER PRIMARY KEY, course_id INTEGER, mode TEXT, total INTEGER, first_try_correct INTEGER,
    xp INTEGER, seconds INTEGER, created_at TEXT DEFAULT (datetime('now'))
);
CREATE VIRTUAL TABLE IF NOT EXISTS pages_fts USING fts5(text, material_id UNINDEXED, page UNINDEXED);
"#;

pub type Shared = Arc<Db>;

pub struct Db {
    conn: Mutex<Connection>,
    pub data_dir: PathBuf,
}

/// Per-user data folder: ~/.local/share/lumi, %APPDATA%\Lumi, ~/Library/Application Support/Lumi.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("LUMI_DATA_DIR") {
        return PathBuf::from(d);
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(if cfg!(target_os = "linux") { "lumi" } else { "Lumi" })
}

impl Db {
    pub fn open() -> rusqlite::Result<Shared> {
        Self::open_at(&data_dir())
    }

    pub fn open_at(dir: &Path) -> rusqlite::Result<Shared> {
        for sub in ["files", "chat_images", "excerpts", "chunks"] {
            let _ = std::fs::create_dir_all(dir.join(sub));
        }
        let path = dir.join("lumi.db");
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt; // the database holds API keys: keep it private
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Arc::new(Db { conn: Mutex::new(conn), data_dir: dir.to_path_buf() }))
    }

    pub fn exec(&self, sql: &str, args: &[&dyn ToSql]) -> i64 {
        let c = self.conn.lock().unwrap();
        if let Err(e) = c.execute(sql, args) {
            eprintln!("[lumi] db error: {e} in {sql}");
        }
        c.last_insert_rowid()
    }

    pub fn query<T>(&self, sql: &str, args: &[&dyn ToSql], mut f: impl FnMut(&Row) -> rusqlite::Result<T>) -> Vec<T> {
        let c = self.conn.lock().unwrap();
        let mut stmt = match c.prepare_cached(sql) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[lumi] db error: {e} in {sql}");
                return vec![];
            }
        };
        let rows = stmt.query_map(args, |r| f(r));
        match rows {
            Ok(it) => it.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                eprintln!("[lumi] db error: {e} in {sql}");
                vec![]
            }
        }
    }

    pub fn one<T>(&self, sql: &str, args: &[&dyn ToSql], f: impl FnMut(&Row) -> rusqlite::Result<T>) -> Option<T> {
        self.query(sql, args, f).into_iter().next()
    }

    pub fn int(&self, sql: &str, args: &[&dyn ToSql]) -> i64 {
        self.one(sql, args, |r| r.get::<_, Option<i64>>(0)).flatten().unwrap_or(0)
    }

    /// Run several statements atomically.
    pub fn tx(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<()>) {
        let mut c = self.conn.lock().unwrap();
        match c.transaction() {
            Ok(t) => {
                if let Err(e) = f(&t).and_then(|_| t.commit()) {
                    eprintln!("[lumi] db transaction failed: {e}");
                }
            }
            Err(e) => eprintln!("[lumi] db error: {e}"),
        }
    }

    // ----- settings (values are JSON, like earlier versions) -----

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let c = self.conn.lock().unwrap();
        let v: Option<String> = c
            .query_row("SELECT value FROM settings WHERE key=?", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        v.and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn get_or<T: DeserializeOwned>(&self, key: &str, default: T) -> T {
        self.get(key).unwrap_or(default)
    }

    pub fn set<T: Serialize>(&self, key: &str, value: &T) {
        let v = serde_json::to_string(value).unwrap_or_else(|_| "null".into());
        self.exec(
            "INSERT INTO settings(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            &[&key, &v],
        );
    }

    pub fn path(&self, sub: &str) -> PathBuf {
        self.data_dir.join(sub)
    }
}

fn migrate(c: &Connection) -> rusqlite::Result<()> {
    let cols = |table: &str| -> rusqlite::Result<Vec<String>> {
        let mut s = c.prepare(&format!("PRAGMA table_info({table})"))?;
        let v = s.query_map([], |r| r.get::<_, String>(1))?.filter_map(|x| x.ok()).collect();
        Ok(v)
    };
    if !cols("sessions")?.contains(&"test_id".to_string()) {
        c.execute("ALTER TABLE sessions ADD COLUMN test_id INTEGER", [])?;
    }
    let m = cols("materials")?;
    for (col, decl) in [("gemini_file", "TEXT"), ("outline", "TEXT"), ("page_labels", "TEXT"),
                        ("indexed", "INTEGER DEFAULT 0"), ("chunked", "INTEGER DEFAULT 0"), ("fts", "INTEGER DEFAULT 0")] {
        if !m.iter().any(|x| x == col) {
            c.execute(&format!("ALTER TABLE materials ADD COLUMN {col} {decl}"), [])?;
        }
    }
    // Older versions stored the AI choice as "gemini"; it is now the multi-AI free mode.
    c.execute("UPDATE settings SET value='\"free\"' WHERE key='provider' AND value='\"gemini\"'", [])?;
    Ok(())
}

// ----- keys & mode -----

pub const FREE_PROVIDERS: [&str; 4] = ["gemini", "groq", "mistral", "openrouter"];

fn env_name(p: &str) -> &'static str {
    match p {
        "gemini" => "GEMINI_API_KEY",
        "groq" => "GROQ_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        _ => "ANTHROPIC_API_KEY",
    }
}

impl Db {
    /// API key for a provider ("claude" or a free one). Environment variables win over saved keys.
    pub fn key(&self, provider: &str) -> Option<String> {
        if let Ok(k) = std::env::var(env_name(provider)) {
            if !k.trim().is_empty() {
                return Some(k);
            }
        }
        let name = if provider == "claude" { "api_key".to_string() } else { format!("{provider}_key") };
        self.get::<Option<String>>(&name).flatten().filter(|k| !k.trim().is_empty())
    }

    pub fn key_from_env(&self, provider: &str) -> bool {
        std::env::var(env_name(provider)).map(|k| !k.trim().is_empty()).unwrap_or(false)
    }

    /// "free" (Gemini + Groq + Mistral + OpenRouter) or "claude" (paid).
    pub fn mode(&self) -> String {
        match self.get::<String>("provider").as_deref() {
            Some("claude") => "claude".into(),
            Some(_) => "free".into(),
            None => "free".into(),
        }
    }

    pub fn has_key(&self) -> bool {
        if self.mode() == "free" {
            FREE_PROVIDERS.iter().any(|p| self.key(p).is_some())
        } else {
            self.key("claude").is_some()
        }
    }
}
