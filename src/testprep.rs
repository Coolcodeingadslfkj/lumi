//! Test prep: a guided path through every concept on an upcoming test.
//! Per concept: quick check (test out if you already know it) -> lesson -> practice until
//! ready. When everything is ready: a full mock exam; weak spots go back to practice.

use crate::db::Db;
use crate::game::{self, Events};
use crate::model::Test;
use chrono::Local;
use serde_json::Value;
use std::collections::HashMap;

pub const READY_MASTERY: f64 = 60.0;
pub const PRACTICE_PASS: f64 = 0.8;
pub const MOCK_WEAK: f64 = 0.7;

#[derive(Clone, Debug)]
pub struct TestConcept {
    pub concept_id: i64,
    pub idx: i64,
    pub priority: String,
    pub why: String,
    pub status: String,
    pub lesson_done: bool,
    pub ready_at: Option<String>,
    pub tested_out: bool,
    pub title: String,
    pub summary: String,
    pub mastery: f64,
    pub ready: bool,
}

pub fn concepts(db: &Db, test_id: i64) -> Vec<TestConcept> {
    db.query(
        "SELECT tc.concept_id, tc.idx, tc.priority, tc.why, tc.status, tc.lesson_done, tc.ready_at, tc.tested_out, c.title, c.summary, c.mastery \
         FROM test_concepts tc JOIN concepts c ON c.id = tc.concept_id WHERE tc.test_id=? ORDER BY tc.idx",
        &[&test_id],
        |r| {
            let status: String = r.get::<_, Option<String>>(4)?.unwrap_or_else(|| "todo".into());
            let mastery: f64 = r.get::<_, Option<f64>>(10)?.unwrap_or(0.0);
            Ok(TestConcept {
                concept_id: r.get(0)?,
                idx: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                priority: r.get::<_, Option<String>>(2)?.unwrap_or_else(|| "medium".into()),
                why: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                ready: status == "ready" || mastery >= READY_MASTERY,
                status,
                lesson_done: r.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,
                ready_at: r.get(6)?,
                tested_out: r.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
                title: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
                summary: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
                mastery,
            })
        },
    )
}

pub fn days_left(t: &Test) -> Option<i64> {
    let d = game::date_of(t.test_date.as_deref()?)?;
    Some((d - Local::now().date_naive()).num_days())
}

fn weight(p: &str) -> f64 {
    match p {
        "high" => 3.0,
        "low" => 1.0,
        _ => 2.0,
    }
}

pub fn readiness(rows: &[TestConcept]) -> i64 {
    if rows.is_empty() {
        return 0;
    }
    let total: f64 = rows.iter().map(|r| weight(&r.priority)).sum();
    let got: f64 = rows.iter().map(|r| weight(&r.priority) * if r.ready { 100.0 } else { r.mastery.min(95.0) }).sum();
    (got / total).round() as i64
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Analyze,
    Diagnostic(i64),
    Lesson(i64),
    Practice(i64),
    Mock,
    Done,
}

pub fn next_step(t: &Test, rows: &[TestConcept]) -> Step {
    for r in rows {
        if r.ready {
            continue;
        }
        if r.status == "todo" {
            return Step::Diagnostic(r.concept_id);
        }
        if !r.lesson_done {
            return Step::Lesson(r.concept_id);
        }
        return Step::Practice(r.concept_id);
    }
    if rows.is_empty() {
        return Step::Analyze;
    }
    match t.mock_last {
        Some(s) if s >= 80 => Step::Done,
        _ => Step::Mock,
    }
}

/// (target, done today): spread remaining concepts over the days left, keeping the last day for a mock exam.
pub fn daily_plan(t: &Test, rows: &[TestConcept]) -> (i64, i64) {
    let remaining = rows.iter().filter(|r| !r.ready).count() as i64;
    let today = game::today();
    let done = rows.iter().filter(|r| r.ready_at.as_deref() == Some(today.as_str())).count() as i64;
    match days_left(t) {
        Some(left) if left >= 0 => {
            let study_days = if left > 1 { (left - 1).max(1) } else { 1 };
            (done + (remaining + study_days - 1) / study_days, done)
        }
        _ => (remaining, done),
    }
}

/// Store the AI's analysis: reuse matching course concepts, add new ones under a test-prep unit.
pub fn save_plan(db: &Db, t: &Test, course_id: i64, data: &Value) {
    let existing = crate::model::Concept::for_course(db, course_id);
    let old: HashMap<i64, TestConcept> = concepts(db, t.id).into_iter().map(|c| (c.concept_id, c)).collect();
    db.exec("DELETE FROM test_concepts WHERE test_id=?", &[&t.id]);
    let unit_idx = 1000 + t.id; // sorts after the regular learning path
    let mut n_new = existing.iter().filter(|c| c.unit_idx == unit_idx).count() as i64;
    let mut seen = vec![];
    let list = data["concepts"].as_array().cloned().unwrap_or_default();
    for (i, c) in list.iter().enumerate() {
        let title = c["title"].as_str().unwrap_or("Concept").to_string();
        let summary = c["summary"].as_str().unwrap_or("").to_string();
        let ex_id = c["existing_concept_id"].as_i64().unwrap_or(0);
        let found = existing.iter().find(|e| e.id == ex_id).or_else(|| existing.iter().find(|e| e.title.to_lowercase() == title.to_lowercase()));
        let cid = match found {
            Some(e) => e.id,
            None => {
                let unit = format!("Test prep: {}", t.name);
                let id = db.exec(
                    "INSERT INTO concepts(course_id, unit_idx, unit_title, idx, title, summary) VALUES(?,?,?,?,?,?)",
                    &[&course_id, &unit_idx, &unit, &n_new, &title, &summary],
                );
                n_new += 1;
                id
            }
        };
        if seen.contains(&cid) {
            continue;
        }
        seen.push(cid);
        let prev = old.get(&cid);
        let idx = i as i64;
        let prio = c["priority"].as_str().unwrap_or("medium").to_string();
        let why = c["why"].as_str().unwrap_or("").to_string();
        let status = prev.map(|p| p.status.clone()).unwrap_or_else(|| "todo".into());
        let lesson = prev.map(|p| p.lesson_done as i64).unwrap_or(0);
        let ready_at = prev.and_then(|p| p.ready_at.clone());
        let tested = prev.map(|p| p.tested_out as i64).unwrap_or(0);
        db.exec(
            "INSERT INTO test_concepts(test_id, concept_id, idx, priority, why, status, lesson_done, ready_at, tested_out) VALUES(?,?,?,?,?,?,?,?,?)",
            &[&t.id, &cid, &idx, &prio, &why, &status, &lesson, &ready_at, &tested],
        );
    }
    let tips = serde_json::to_string(&data["tips"]).unwrap_or_else(|_| "[]".into());
    let overview = data["overview"].as_str().unwrap_or("").to_string();
    db.exec("UPDATE tests SET overview=?, tips=?, analyzed=1 WHERE id=?", &[&overview, &tips, &t.id]);
}

/// Update test progress after a quiz. Returns (message, mock score).
pub fn record_quiz(db: &Db, test_id: i64, kind: &str, results: &HashMap<i64, Vec<bool>>, ev: &mut Events) -> (String, Option<i64>) {
    let today = game::today();
    let ready = READY_MASTERY;
    if kind == "mock" {
        let first: Vec<bool> = results.values().flatten().copied().collect();
        let score = if first.is_empty() { 0 } else { (100.0 * first.iter().filter(|x| **x).count() as f64 / first.len() as f64).round() as i64 };
        let best_before: Option<i64> = db.one("SELECT mock_best FROM tests WHERE id=?", &[&test_id], |r| r.get(0)).flatten();
        db.exec("UPDATE tests SET mock_last=?, mock_best=MAX(COALESCE(mock_best,0), ?) WHERE id=?", &[&score, &score, &test_id]);
        let mut weak = vec![];
        for (cid, rs) in results {
            if !rs.is_empty() && (rs.iter().filter(|x| **x).count() as f64 / rs.len() as f64) < MOCK_WEAK {
                db.exec("UPDATE test_concepts SET status='learning', lesson_done=1, ready_at=NULL WHERE test_id=? AND concept_id=?", &[&test_id, cid]);
                db.exec("UPDATE concepts SET mastery=MIN(mastery, ?) WHERE id=?", &[&(ready - 1.0), cid]);
                if let Some(c) = crate::model::Concept::get(db, *cid) {
                    weak.push(c.title);
                }
            }
        }
        if score >= 90 {
            game::unlock(db, "mock_ace", ev);
        }
        let mut msg = format!("Mock exam score: {score}%");
        if let Some(b) = best_before {
            msg += &format!(" (best so far: {}%)", b.max(score));
        }
        if !weak.is_empty() {
            msg += &format!(" · Back to practice: {}", weak.join(", "));
        } else if score >= 80 {
            msg += " · You're ready for this test. 🎓";
        }
        return (msg, Some(score));
    }
    let mut msgs = vec![];
    for (cid, rs) in results {
        if rs.is_empty() {
            continue;
        }
        let acc = rs.iter().filter(|x| **x).count() as f64 / rs.len() as f64;
        if db.int("SELECT COUNT(*) FROM test_concepts WHERE test_id=? AND concept_id=?", &[&test_id, cid]) == 0 {
            continue;
        }
        let title = crate::model::Concept::get(db, *cid).map(|c| c.title).unwrap_or_default();
        if kind == "diagnostic" {
            if acc >= 1.0 && rs.len() >= 3 {
                db.exec("UPDATE test_concepts SET status='ready', tested_out=1, ready_at=? WHERE test_id=? AND concept_id=?", &[&today, &test_id, cid]);
                db.exec("UPDATE concepts SET mastery=MAX(mastery, ?) WHERE id=?", &[&ready, cid]);
                game::unlock(db, "tested_out", ev);
                msgs.push(format!("⚡ You tested out of “{title}”, so we'll skip it."));
            } else {
                db.exec("UPDATE test_concepts SET status='learning' WHERE test_id=? AND concept_id=?", &[&test_id, cid]);
                msgs.push(format!("Next: a quick lesson on “{title}” to fill the gaps."));
            }
        } else {
            db.exec("UPDATE test_concepts SET lesson_done=1 WHERE test_id=? AND concept_id=?", &[&test_id, cid]);
            if acc >= PRACTICE_PASS {
                db.exec("UPDATE test_concepts SET status='ready', ready_at=? WHERE test_id=? AND concept_id=?", &[&today, &test_id, cid]);
                db.exec("UPDATE concepts SET mastery=MAX(mastery, ?) WHERE id=?", &[&ready, cid]);
                msgs.push(format!("✅ “{title}” is test-ready!"));
            } else {
                db.exec("UPDATE test_concepts SET status='learning' WHERE test_id=? AND concept_id=?", &[&test_id, cid]);
                msgs.push(format!("“{title}” needs one more round. You need {}% to mark it ready.", (PRACTICE_PASS * 100.0) as i64));
            }
        }
    }
    let rows = concepts(db, test_id);
    if !rows.is_empty() && rows.iter().all(|r| r.ready) {
        game::unlock(db, "test_ready", ev);
        msgs.push("Every concept is ready. Time for a full mock exam!".into());
    }
    (msgs.join(" "), None)
}
