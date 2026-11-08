//! Rows from the database as plain structs.

use crate::db::Db;
use rusqlite::Row;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct Course {
    pub id: i64,
    pub name: String,
    pub emoji: String,
    pub color: String,
    pub goal: String,
}

impl Course {
    const COLS: &'static str = "id, name, emoji, color, goal";
    fn from_row(r: &Row) -> rusqlite::Result<Self> {
        Ok(Course {
            id: r.get(0)?,
            name: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            emoji: r.get::<_, Option<String>>(2)?.filter(|s| !s.is_empty()).unwrap_or_else(|| "📘".into()),
            color: r.get::<_, Option<String>>(3)?.unwrap_or_else(|| "#7c5cff".into()),
            goal: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        })
    }
    pub fn all(db: &Db) -> Vec<Course> {
        db.query(&format!("SELECT {} FROM courses ORDER BY id", Self::COLS), &[], Self::from_row)
    }
    pub fn get(db: &Db, id: i64) -> Option<Course> {
        db.one(&format!("SELECT {} FROM courses WHERE id=?", Self::COLS), &[&id], Self::from_row)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Material {
    pub id: i64,
    pub course_id: i64,
    pub filename: String,
    pub kind: String, // pdf | image | text
    pub mime: String,
    pub path: Option<String>,
    pub text: Option<String>,
    pub size: i64,
    pub pages: i64,
    pub enabled: bool,
    pub outline: Option<String>,
    pub page_labels: Option<String>,
    pub indexed: bool,
    pub chunked: bool,
}

impl Material {
    const COLS: &'static str =
        "id, course_id, filename, kind, mime, path, text, size, pages, enabled, outline, page_labels, indexed, chunked";
    fn from_row(r: &Row) -> rusqlite::Result<Self> {
        Ok(Material {
            id: r.get(0)?,
            course_id: r.get(1)?,
            filename: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            kind: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            mime: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            path: r.get(5)?,
            text: r.get(6)?,
            size: r.get::<_, Option<i64>>(7)?.unwrap_or(0),
            pages: r.get::<_, Option<i64>>(8)?.unwrap_or(0),
            enabled: r.get::<_, Option<i64>>(9)?.unwrap_or(1) != 0,
            outline: r.get(10)?,
            page_labels: r.get(11)?,
            indexed: r.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
            chunked: r.get::<_, Option<i64>>(13)?.unwrap_or(0) != 0,
        })
    }
    pub fn get(db: &Db, id: i64) -> Option<Material> {
        db.one(&format!("SELECT {} FROM materials WHERE id=?", Self::COLS), &[&id], Self::from_row)
    }
    /// Without the (possibly large) pasted text, for lists.
    pub fn for_course(db: &Db, course_id: i64) -> Vec<Material> {
        db.query(
            &format!("SELECT {} FROM materials WHERE course_id=? ORDER BY id", Self::COLS.replace(" text,", " NULL,")),
            &[&course_id],
            Self::from_row,
        )
    }
    pub fn enabled_ids(db: &Db, course_id: i64) -> Vec<i64> {
        db.query("SELECT id FROM materials WHERE course_id=? AND enabled=1 ORDER BY id", &[&course_id], |r| r.get(0))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Concept {
    pub id: i64,
    pub course_id: i64,
    pub unit_idx: i64,
    pub unit_title: String,
    pub idx: i64,
    pub title: String,
    pub summary: String,
    pub mastery: f64,
    pub bx: i64,
    pub due_at: Option<String>,
    pub attempts: i64,
}

impl Concept {
    const COLS: &'static str = "id, course_id, unit_idx, unit_title, idx, title, summary, mastery, box, due_at, attempts";
    fn from_row(r: &Row) -> rusqlite::Result<Self> {
        Ok(Concept {
            id: r.get(0)?,
            course_id: r.get(1)?,
            unit_idx: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            unit_title: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            idx: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
            title: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            summary: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            mastery: r.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
            bx: r.get::<_, Option<i64>>(8)?.unwrap_or(0),
            due_at: r.get(9)?,
            attempts: r.get::<_, Option<i64>>(10)?.unwrap_or(0),
        })
    }
    pub fn level(&self) -> i64 {
        crate::game::mastery_level(self.mastery)
    }
    pub fn get(db: &Db, id: i64) -> Option<Concept> {
        db.one(&format!("SELECT {} FROM concepts WHERE id=?", Self::COLS), &[&id], Self::from_row)
    }
    pub fn for_course(db: &Db, course_id: i64) -> Vec<Concept> {
        db.query(&format!("SELECT {} FROM concepts WHERE course_id=? ORDER BY unit_idx, idx", Self::COLS), &[&course_id], Self::from_row)
    }
    pub fn many(db: &Db, ids: &[i64]) -> Vec<Concept> {
        ids.iter().filter_map(|id| Self::get(db, *id)).collect()
    }
    pub fn query(db: &Db, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> Vec<Concept> {
        db.query(&format!("SELECT {} FROM concepts WHERE {where_sql}", Self::COLS), args, Self::from_row)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub id: i64,
    pub course_id: i64,
    pub concept_id: Option<i64>,
    pub title: String,
    pub material_ids: Vec<i64>,
    pub messages: Vec<serde_json::Value>,
    pub test_id: Option<i64>,
    pub updated_at: String,
}

impl Session {
    pub fn get(db: &Db, id: i64) -> Option<Session> {
        db.one(
            "SELECT id, course_id, concept_id, title, material_ids, messages, test_id, updated_at FROM sessions WHERE id=?",
            &[&id],
            |r| {
                Ok(Session {
                    id: r.get(0)?,
                    course_id: r.get(1)?,
                    concept_id: r.get(2)?,
                    title: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    material_ids: serde_json::from_str(&r.get::<_, Option<String>>(4)?.unwrap_or_default()).unwrap_or_default(),
                    messages: serde_json::from_str(&r.get::<_, Option<String>>(5)?.unwrap_or_default()).unwrap_or_default(),
                    test_id: r.get(6)?,
                    updated_at: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                })
            },
        )
    }
    /// (id, title, concept_id, updated_at) for a course's lesson list.
    pub fn list(db: &Db, course_id: i64) -> Vec<(i64, String, Option<i64>, String)> {
        db.query(
            "SELECT id, title, concept_id, updated_at FROM sessions WHERE course_id=? ORDER BY updated_at DESC LIMIT 30",
            &[&course_id],
            |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get(2)?, r.get::<_, Option<String>>(3)?.unwrap_or_default())),
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct Test {
    pub id: i64,
    pub course_id: i64,
    pub name: String,
    pub test_date: Option<String>,
    pub topics: String,
    pub material_ids: Vec<i64>,
    pub overview: String,
    pub tips: Vec<String>,
    pub analyzed: bool,
    pub mock_best: Option<i64>,
    pub mock_last: Option<i64>,
}

impl Test {
    const COLS: &'static str = "id, course_id, name, test_date, topics, material_ids, overview, tips, analyzed, mock_best, mock_last";
    fn from_row(r: &Row) -> rusqlite::Result<Self> {
        Ok(Test {
            id: r.get(0)?,
            course_id: r.get(1)?,
            name: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            test_date: r.get::<_, Option<String>>(3)?.filter(|s| !s.is_empty()),
            topics: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            material_ids: serde_json::from_str(&r.get::<_, Option<String>>(5)?.unwrap_or_default()).unwrap_or_default(),
            overview: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            tips: serde_json::from_str(&r.get::<_, Option<String>>(7)?.unwrap_or_default()).unwrap_or_default(),
            analyzed: r.get::<_, Option<i64>>(8)?.unwrap_or(0) != 0,
            mock_best: r.get(9)?,
            mock_last: r.get(10)?,
        })
    }
    pub fn get(db: &Db, id: i64) -> Option<Test> {
        db.one(&format!("SELECT {} FROM tests WHERE id=?", Self::COLS), &[&id], Self::from_row)
    }
    pub fn all(db: &Db) -> Vec<Test> {
        db.query(&format!("SELECT {} FROM tests ORDER BY (test_date IS NULL), test_date, id", Self::COLS), &[], Self::from_row)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Mistake {
    pub id: i64,
    pub course_id: i64,
    pub concept_id: Option<i64>,
    pub question: serde_json::Value,
    pub user_answer: String,
    pub times_missed: i64,
    pub updated_at: String,
}

impl Mistake {
    pub fn unfixed(db: &Db, course_id: Option<i64>, limit: i64) -> Vec<Mistake> {
        let sql = format!(
            "SELECT id, course_id, concept_id, question, user_answer, times_missed, updated_at FROM mistakes WHERE fixed=0 {} ORDER BY updated_at DESC LIMIT ?",
            if course_id.is_some() { "AND course_id=?" } else { "" }
        );
        let f = |r: &Row| {
            Ok(Mistake {
                id: r.get(0)?,
                course_id: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                concept_id: r.get(2)?,
                question: serde_json::from_str(&r.get::<_, Option<String>>(3)?.unwrap_or_default()).unwrap_or_default(),
                user_answer: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                times_missed: r.get::<_, Option<i64>>(5)?.unwrap_or(1),
                updated_at: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            })
        };
        match course_id {
            Some(c) => db.query(&sql, &[&c, &limit], f),
            None => db.query(&sql, &[&limit], f),
        }
    }
}
