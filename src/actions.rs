//! Learning actions that combine storage and game rules (answering, finishing sessions, ...).

use crate::db::Db;
use crate::game::{self, Events};
use crate::model::{Concept, Material, Test};
use crate::testprep;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct Answer<'a> {
    pub concept_id: Option<i64>,
    pub course_id: Option<i64>,
    pub correct: bool,
    pub combo: i64,
    pub retry: bool,
    pub question: Option<&'a Value>,
    pub user_answer: &'a str,
    pub mistake_id: Option<i64>,
    pub from_check_card: bool,
}

fn save_mistake(db: &Db, a: &Answer) {
    let Some(q) = a.question else { return };
    let course_id = a.course_id.or_else(|| a.concept_id.and_then(|c| Concept::get(db, c)).map(|c| c.course_id));
    let Some(course_id) = course_id else { return };
    let prompt = q["prompt"].as_str().unwrap_or("").to_string();
    let same: Option<i64> = db.one("SELECT id FROM mistakes WHERE course_id=? AND fixed=0 AND json_extract(question, '$.prompt')=?", &[&course_id, &prompt], |r| r.get(0));
    let ua: String = a.user_answer.chars().take(500).collect();
    match same {
        Some(id) => {
            db.exec("UPDATE mistakes SET times_missed=times_missed+1, user_answer=?, updated_at=datetime('now') WHERE id=?", &[&ua, &id]);
        }
        None => {
            db.exec("INSERT INTO mistakes(course_id, concept_id, question, user_answer) VALUES(?,?,?,?)",
                &[&course_id, &a.concept_id.filter(|c| *c > 0), &q.to_string(), &ua]);
        }
    }
}

/// Record one answer: XP, mastery, mistake notebook, quests, achievements.
pub fn answer(db: &Db, a: &Answer) -> Events {
    let mut fixed_now = false;
    if !a.correct && !a.retry && a.question.is_some() && a.mistake_id.is_none() {
        save_mistake(db, a);
    }
    if let Some(mid) = a.mistake_id {
        if a.correct && !a.retry && db.int("SELECT COUNT(*) FROM mistakes WHERE id=? AND fixed=0", &[&mid]) > 0 {
            db.exec("UPDATE mistakes SET fixed=1, fixed_at=datetime('now') WHERE id=?", &[&mid]);
            game::quest_bump(db, "fixed", 1);
            fixed_now = true;
        } else if !a.correct {
            db.exec("UPDATE mistakes SET times_missed=times_missed+1, updated_at=datetime('now') WHERE id=?", &[&mid]);
        }
    }
    if a.correct {
        game::quest_bump(db, "correct", 1);
        game::quest_max(db, "combo", a.combo);
    }
    if a.from_check_card {
        game::quest_bump(db, "checks", 1);
    }
    if let (Some(q), Some(course)) = (a.question, a.course_id.or_else(|| a.concept_id.and_then(|c| Concept::get(db, c)).map(|c| c.course_id))) {
        game::bank_question(db, course, a.concept_id, q);
    }
    let xp = if a.correct && !a.retry { 10 + (2 * (a.combo - 1).max(0)).min(10) } else if a.correct { 5 } else { 0 };
    let mut ev = game::award(db, xp, 1, a.correct as i64);
    if let Some(cid) = a.concept_id.filter(|c| *c > 0) {
        if let Some(m) = game::record_answer(db, cid, a.correct) {
            if m.after == 5 {
                game::unlock(db, "mastered", &mut ev);
            }
        }
    }
    if a.correct && a.combo >= 5 {
        game::unlock(db, "combo_5", &mut ev);
    }
    if a.correct && a.combo >= 15 {
        game::unlock(db, "combo_15", &mut ev);
    }
    if a.correct && a.retry && game::counter(db, "comebacks") >= 10 {
        game::unlock(db, "comeback", &mut ev);
    }
    if fixed_now {
        ev.mistake_fixed = true;
        let n = game::counter(db, "fixed");
        if n >= 10 {
            game::unlock(db, "fixer_10", &mut ev);
        }
        if n >= 50 {
            game::unlock(db, "fixer_50", &mut ev);
        }
    }
    ev
}

pub struct Finish<'a> {
    pub course_id: i64,
    pub results: &'a HashMap<i64, Vec<bool>>, // concept -> first-try results
    pub seconds: i64,
    pub mode: &'a str,      // practice | challenge | boss
    pub kind: Option<&'a str>, // diagnostic | practice | mock | mistakes | None
    pub review: bool,
    pub hearts_left: i64,
    pub test_id: Option<i64>,
    pub boss: Option<(i64, bool)>, // (unit_idx, won)
}

pub struct FinishOut {
    pub events: Events,
    pub bonus: i64,
    pub perfect: bool,
    pub test_message: Option<String>,
    pub mock_score: Option<i64>,
    pub concepts: Vec<Concept>,
}

pub fn finish(db: &Db, f: &Finish) -> FinishOut {
    let first: Vec<bool> = f.results.values().flatten().copied().collect();
    let perfect = !first.is_empty() && first.iter().all(|x| *x);
    for (cid, rs) in f.results {
        if !rs.is_empty() && *cid > 0 {
            game::schedule(db, *cid, rs.iter().filter(|x| **x).count() as f64 / rs.len() as f64);
        }
    }
    let won_boss = f.boss.map(|b| b.1).unwrap_or(false);
    let bonus = 20 + if perfect { 15 } else { 0 } + if f.mode == "challenge" && f.hearts_left > 0 { 10 } else { 0 } + if won_boss { 30 } else { 0 };
    game::quest_bump(db, "sessions", 1);
    if perfect {
        game::quest_bump(db, "perfect", 1);
    }
    if f.review {
        game::quest_bump(db, "review", 1);
    }
    if won_boss {
        game::quest_bump(db, "boss", 1);
    }
    let mut ev = game::award(db, bonus, 0, 0);
    if perfect {
        game::add_gems(db, 5, &mut ev);
    }
    if let Some((unit, true)) = f.boss {
        let first_win = db.int("SELECT COUNT(*) FROM bosses WHERE course_id=? AND unit_idx=?", &[&f.course_id, &unit]) == 0;
        db.exec("INSERT OR IGNORE INTO bosses(course_id, unit_idx) VALUES(?,?)", &[&f.course_id, &unit]);
        game::add_gems(db, if first_win { 25 } else { 10 }, &mut ev);
        game::unlock(db, "boss_1", &mut ev);
        if db.int("SELECT COUNT(*) FROM bosses", &[]) >= 5 {
            game::unlock(db, "boss_5", &mut ev);
        }
    }
    let (total, right) = (first.len() as i64, first.iter().filter(|x| **x).count() as i64);
    db.exec("INSERT INTO quizzes(course_id, mode, total, first_try_correct, xp, seconds) VALUES(?,?,?,?,?,?)",
        &[&f.course_id, &f.mode, &total, &right, &bonus, &f.seconds]);
    game::unlock(db, "first_steps", &mut ev);
    if perfect {
        game::unlock(db, "flawless", &mut ev);
    }
    if f.review && game::counter(db, "reviews") >= 5 {
        game::unlock(db, "reviewer", &mut ev);
    }
    let course_concepts = Concept::for_course(db, f.course_id);
    if !course_concepts.is_empty() && course_concepts.iter().all(|c| c.level() >= 3) {
        game::unlock(db, "course_master", &mut ev);
    }
    let (mut test_message, mut mock_score) = (None, None);
    if let Some(tid) = f.test_id {
        let (m, s) = testprep::record_quiz(db, tid, f.kind.unwrap_or("practice"), f.results, &mut ev);
        test_message = Some(m).filter(|m| !m.is_empty());
        mock_score = s;
    }
    let ids: Vec<i64> = f.results.keys().copied().filter(|c| *c > 0).collect();
    FinishOut { events: ev, bonus, perfect, test_message, mock_score, concepts: Concept::many(db, &ids) }
}

/// One-time: put check questions from past lessons into the Lightning question bank.
pub fn seed_bank_from_lessons(db: &Db) {
    if db.get_or("bank_seeded_lessons", false) {
        return;
    }
    let ids = db.query("SELECT id FROM sessions", &[], |r| r.get::<_, i64>(0));
    for s in ids.into_iter().filter_map(|id| crate::model::Session::get(db, id)) {
        for m in s.messages.iter().filter(|m| m["role"] == "assistant") {
            let text: String = m["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect();
            for seg in crate::ui::md::segments(&text, false) {
                if let crate::ui::md::Segment::Check(q) = seg {
                    let opts = q["options"].clone();
                    let q = json!({"type": "multiple_choice", "prompt": q["question"], "options": opts, "correct_index": q["answer"], "explanation": q["explanation"]});
                    game::bank_question(db, s.course_id, s.concept_id, &q);
                }
            }
        }
    }
    db.set("bank_seeded_lessons", &true);
}

pub fn new_course(db: &Db, name: &str, goal: &str, emoji: &str) -> i64 {
    const COLORS: [&str; 6] = ["#7c5cff", "#ff6b9a", "#22c58b", "#ffb020", "#2fa8ff", "#ff7a45"];
    let n = db.int("SELECT COUNT(*) FROM courses", &[]) as usize;
    let name: String = if name.trim().is_empty() { "My course".into() } else { name.trim().chars().take(80).collect() };
    let goal: String = goal.trim().chars().take(300).collect();
    db.exec("INSERT INTO courses(name, goal, emoji, color) VALUES(?,?,?,?)", &[&name, &goal, &emoji, &COLORS[n % COLORS.len()]])
}

pub fn delete_course(db: &Db, cid: i64) {
    for mid in db.query("SELECT id FROM materials WHERE course_id=?", &[&cid], |r| r.get::<_, i64>(0)) {
        crate::materials::delete(db, mid);
    }
    for tid in db.query("SELECT id FROM tests WHERE course_id=?", &[&cid], |r| r.get::<_, i64>(0)) {
        delete_test(db, tid);
    }
    for t in ["concepts", "sessions", "mistakes", "bosses", "question_bank", "notes"] {
        db.exec(&format!("DELETE FROM {t} WHERE course_id=?"), &[&cid]);
    }
    db.exec("DELETE FROM courses WHERE id=?", &[&cid]);
}

pub fn delete_test(db: &Db, tid: i64) {
    db.exec("DELETE FROM test_concepts WHERE test_id=?", &[&tid]);
    db.exec("DELETE FROM tests WHERE id=?", &[&tid]);
}

/// Start a lesson. A test-prep lesson uses the test's materials.
pub fn new_session(db: &Db, course_id: i64, concept_id: Option<i64>, test_id: Option<i64>) -> i64 {
    let concept = concept_id.and_then(|c| Concept::get(db, c));
    let mut title = concept.map(|c| c.title).unwrap_or_else(|| "Open study session".into());
    let test = test_id.and_then(|t| Test::get(db, t));
    if test.is_some() {
        title = format!("Test prep · {title}");
    }
    let mids = test.map(|t| t.material_ids).filter(|m| !m.is_empty()).unwrap_or_else(|| Material::enabled_ids(db, course_id));
    db.exec("INSERT INTO sessions(course_id, concept_id, title, material_ids, test_id) VALUES(?,?,?,?,?)",
        &[&course_id, &concept_id, &title, &json!(mids).to_string(), &test_id])
}

pub fn new_test(db: &Db, course_id: i64, name: &str, date: Option<&str>, topics: &str) -> i64 {
    let name: String = if name.trim().is_empty() { "My test".into() } else { name.trim().chars().take(100).collect() };
    let topics: String = topics.trim().chars().take(20000).collect();
    db.exec("INSERT INTO tests(course_id, name, test_date, topics) VALUES(?,?,?,?)", &[&course_id, &name, &date.filter(|d| !d.is_empty()), &topics])
}
