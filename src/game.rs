//! XP, levels, streaks, spaced repetition, achievements, daily quests, gems and the shop.
//!
//! Learning-science basis: retrieval practice with immediate feedback, spaced review
//! (Leitner boxes), successive relearning (missed questions return), and streaks with
//! freezes so one missed day doesn't end the habit.

use crate::db::Db;
use crate::model::Concept;
use chrono::{Duration, Local, NaiveDate, Timelike};
use rand::RngExt;
use serde::{Deserialize, Serialize};

pub const BOX_INTERVAL_DAYS: [i64; 7] = [0, 1, 3, 7, 14, 30, 60];
pub const MAX_FREEZES: i64 = 2;

pub struct Achievement {
    pub key: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub icon: &'static str,
}

macro_rules! ach {
    ($k:expr, $n:expr, $d:expr, $i:expr) => {
        Achievement { key: $k, name: $n, desc: $d, icon: $i }
    };
}

pub const ACHIEVEMENTS: &[Achievement] = &[
    ach!("first_steps", "First Steps", "Finish your first practice session", "👣"),
    ach!("curious_mind", "Curious Mind", "Have your first lesson with Lumi", "💬"),
    ach!("librarian", "Librarian", "Add 3 study materials", "📚"),
    ach!("pathfinder", "Pathfinder", "Generate your first learning path", "🗺️"),
    ach!("flawless", "Flawless", "Finish a practice session with no mistakes", "💎"),
    ach!("combo_5", "On Fire", "Get 5 answers right in a row", "🔥"),
    ach!("combo_15", "Unstoppable", "Get 15 answers right in a row", "⚡"),
    ach!("streak_3", "Warming Up", "Reach a 3-day streak", "🌱"),
    ach!("streak_7", "One Week Strong", "Reach a 7-day streak", "🌿"),
    ach!("streak_30", "Habit Master", "Reach a 30-day streak", "🌳"),
    ach!("streak_100", "Legend", "Reach a 100-day streak", "🏆"),
    ach!("xp_500", "Rising Star", "Earn 500 XP", "⭐"),
    ach!("xp_2500", "Scholar", "Earn 2,500 XP", "🎓"),
    ach!("xp_10000", "Sage", "Earn 10,000 XP", "🦉"),
    ach!("goal_crusher", "Goal Crusher", "Hit your daily goal 5 times", "🎯"),
    ach!("mastered", "Mastered!", "Bring a concept to full mastery", "👑"),
    ach!("course_master", "Course Conqueror", "Reach level 3+ on every concept in a course", "🏰"),
    ach!("comeback", "Comeback Kid", "Fix 10 mistakes on the second try", "🔁"),
    ach!("night_owl", "Night Owl", "Study after 11 pm", "🌙"),
    ach!("early_bird", "Early Bird", "Study before 7 am", "🌅"),
    ach!("reviewer", "Memory Keeper", "Complete 5 spaced reviews", "🧠"),
    ach!("tested_out", "Already Got It", "Test out of a concept in test prep", "⚡"),
    ach!("test_ready", "Exam Ready", "Get every concept on a test to ready", "📝"),
    ach!("mock_ace", "Mock Exam Ace", "Score 90%+ on a mock exam", "🎓"),
    ach!("boss_1", "Boss Slayer", "Defeat your first unit boss", "⚔️"),
    ach!("boss_5", "Monster Hunter", "Defeat 5 unit bosses", "🐉"),
    ach!("fixer_10", "Learning From Mistakes", "Fix 10 mistakes from your notebook", "📓"),
    ach!("fixer_50", "Mistake Slayer", "Fix 50 mistakes from your notebook", "🗡️"),
    ach!("quest_7", "Questing", "Open 7 daily chests", "🎁"),
    ach!("shopper", "Fresh Look", "Buy something in the shop", "🛍️"),
    ach!("collector", "Collector", "Own 10 items from the shop", "🧺"),
    ach!("evolved", "Glow Up", "Help Lumi evolve", "🌟"),
];

pub fn achievement(key: &str) -> Option<&'static Achievement> {
    ACHIEVEMENTS.iter().find(|a| a.key == key)
}

/// Everything a learning action produced, for celebrations in the UI.
#[derive(Clone, Debug, Default)]
pub struct Events {
    pub xp: i64,
    pub achievements: Vec<&'static str>,
    pub level_up: Option<i64>,
    pub goal_hit: bool,
    pub freeze_earned: bool,
    pub gems: i64,
    pub quests_completed: Vec<String>,
    pub evolved: Option<Stage>,
    pub chest_gems: i64,
    pub chest_freeze: bool,
    pub mistake_fixed: bool,
    /// Shop item id won from a mystery box.
    pub unboxed: Option<String>,
}

impl Events {
    pub fn merge(&mut self, o: Events) {
        self.xp += o.xp;
        self.achievements.extend(o.achievements);
        self.level_up = o.level_up.or(self.level_up);
        self.goal_hit |= o.goal_hit;
        self.freeze_earned |= o.freeze_earned;
        self.gems += o.gems;
        self.quests_completed.extend(o.quests_completed);
        self.evolved = o.evolved.or(self.evolved.take());
        self.chest_gems += o.chest_gems;
        self.chest_freeze |= o.chest_freeze;
        self.mistake_fixed |= o.mistake_fixed;
        self.unboxed = o.unboxed.or(self.unboxed.take());
    }
}

pub fn today() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}

pub fn date_of(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Level n needs 50*n*(n+1) total XP: 100, 300, 600, 1000, ...  Returns (level, into, needed).
pub fn level_for_xp(xp: i64) -> (i64, i64, i64) {
    let n = (((1.0 + 4.0 * xp as f64 / 50.0).sqrt() - 1.0) / 2.0).floor() as i64;
    let cur = 50 * n * (n + 1);
    let nxt = 50 * (n + 1) * (n + 2);
    (n + 1, xp - cur, nxt - cur)
}

pub fn total_xp(db: &Db) -> i64 {
    db.int("SELECT COALESCE(SUM(xp),0) FROM activity", &[])
}

pub fn mastery_level(m: f64) -> i64 {
    if m >= 100.0 { 5 } else { ((m / 20.0).floor() as i64).min(5) }
}

pub fn today_xp(db: &Db) -> i64 {
    db.int("SELECT xp FROM activity WHERE day=?", &[&today()])
}

/// If the learner missed exactly the days a freeze can cover, spend freezes.
pub fn apply_streak_freezes(db: &Db) {
    let Some(last) = db.one("SELECT day FROM activity WHERE xp>0 OR frozen=1 ORDER BY day DESC LIMIT 1", &[], |r| r.get::<_, String>(0)) else { return };
    let Some(last_d) = date_of(&last) else { return };
    let gap = (Local::now().date_naive() - last_d).num_days() - 1;
    let freezes: i64 = db.get_or("freezes", 0);
    if gap > 0 && gap <= freezes {
        for i in 1..=gap {
            let d = (last_d + Duration::days(i)).format("%Y-%m-%d").to_string();
            db.exec("INSERT INTO activity(day, frozen) VALUES(?,1) ON CONFLICT(day) DO UPDATE SET frozen=1", &[&d]);
        }
        db.set("freezes", &(freezes - gap));
        db.set("freeze_used_notice", &gap);
    }
}

pub fn streak(db: &Db) -> i64 {
    let days: std::collections::HashSet<String> = db.query("SELECT day FROM activity WHERE xp>0 OR frozen=1", &[], |r| r.get(0)).into_iter().collect();
    let mut d = Local::now().date_naive();
    if !days.contains(&d.format("%Y-%m-%d").to_string()) {
        d -= Duration::days(1); // today not done yet: the streak is still alive from yesterday
    }
    let mut n = 0;
    while days.contains(&d.format("%Y-%m-%d").to_string()) {
        n += 1;
        d -= Duration::days(1);
    }
    n
}

pub fn unlock(db: &Db, key: &'static str, ev: &mut Events) {
    if achievement(key).is_some() && db.int("SELECT COUNT(*) FROM achievements WHERE key=?", &[&key]) == 0 {
        db.exec("INSERT INTO achievements(key) VALUES(?)", &[&key]);
        ev.achievements.push(key);
    }
}

pub fn counter(db: &Db, key: &str) -> i64 {
    let k = format!("count_{key}");
    let v: i64 = db.get_or(&k, 0) + 1;
    db.set(&k, &v);
    v
}

/// Add XP for today; returns level-up / goal / achievement events.
pub fn award(db: &Db, xp: i64, answers: i64, correct: i64) -> Events {
    let xp = if double_xp_active(db) { xp * 2 } else { xp };
    let mut ev = Events { xp, ..Default::default() };
    let before_total = total_xp(db);
    let before_today = today_xp(db);
    let before_streak = streak(db);
    db.exec(
        "INSERT INTO activity(day, xp, answers, correct) VALUES(?,?,?,?) ON CONFLICT(day) DO UPDATE SET \
         xp=xp+excluded.xp, answers=answers+excluded.answers, correct=correct+excluded.correct",
        &[&today(), &xp, &answers, &correct],
    );
    let after_total = before_total + xp;
    let (lb, la) = (level_for_xp(before_total).0, level_for_xp(after_total).0);
    if la > lb {
        ev.level_up = Some(la);
        add_gems(db, 20, &mut ev);
        if stage_for_level(la).key != stage_for_level(lb).key {
            ev.evolved = Some(stage_for_level(la));
            unlock(db, "evolved", &mut ev);
        }
    }
    let goal: i64 = db.get_or("daily_goal", 50);
    if before_today < goal && goal <= before_today + xp {
        ev.goal_hit = true;
        add_gems(db, 10, &mut ev);
        if counter(db, "goals") >= 5 {
            unlock(db, "goal_crusher", &mut ev);
        }
    }
    let s = streak(db);
    let freezes: i64 = db.get_or("freezes", 0);
    if s > before_streak && s % 7 == 0 && freezes < MAX_FREEZES {
        db.set("freezes", &(freezes + 1));
        ev.freeze_earned = true;
    }
    for (n, k) in [(3, "streak_3"), (7, "streak_7"), (30, "streak_30"), (100, "streak_100")] {
        if s >= n {
            unlock(db, k, &mut ev);
        }
    }
    for (n, k) in [(500, "xp_500"), (2500, "xp_2500"), (10000, "xp_10000")] {
        if after_total >= n {
            unlock(db, k, &mut ev);
        }
    }
    let hour = Local::now().hour();
    if xp > 0 && hour >= 23 {
        unlock(db, "night_owl", &mut ev);
    }
    if xp > 0 && hour < 7 {
        unlock(db, "early_bird", &mut ev);
    }
    quest_check(db, &mut ev);
    ev
}

#[derive(Clone, Debug)]
pub struct MasteryChange {
    pub before: i64,
    pub after: i64,
    pub mastery: f64,
}

pub fn record_answer(db: &Db, concept_id: i64, correct: bool) -> Option<MasteryChange> {
    let c = Concept::get(db, concept_id)?;
    // gains shrink as mastery grows; misses cost more at high mastery
    let m = if correct { c.mastery + (100.0 - c.mastery) * 0.18 } else { c.mastery * 0.85 };
    let m = (m * 10.0).round() / 10.0;
    db.exec(
        "UPDATE concepts SET mastery=?, attempts=attempts+1, correct=correct+?, last_seen=datetime('now') WHERE id=?",
        &[&m, &(correct as i64), &concept_id],
    );
    Some(MasteryChange { before: mastery_level(c.mastery), after: mastery_level(m), mastery: m })
}

/// Leitner step after a practice session on this concept.
pub fn schedule(db: &Db, concept_id: i64, accuracy: f64) {
    let Some(c) = Concept::get(db, concept_id) else { return };
    let max_box = BOX_INTERVAL_DAYS.len() as i64 - 1;
    let bx = if accuracy >= 0.8 { (c.bx + 1).min(max_box) } else if accuracy < 0.5 { (c.bx - 1).max(0) } else { c.bx };
    let days = BOX_INTERVAL_DAYS[bx as usize].max(1);
    let due = (Local::now().date_naive() + Duration::days(days)).format("%Y-%m-%d").to_string();
    db.exec("UPDATE concepts SET box=?, due_at=? WHERE id=?", &[&bx, &due, &concept_id]);
}

pub fn due_concepts(db: &Db, course_id: Option<i64>) -> Vec<Concept> {
    let t = today();
    match course_id {
        Some(c) => Concept::query(db, "due_at IS NOT NULL AND due_at<=? AND course_id=? ORDER BY due_at, mastery", &[&t, &c]),
        None => Concept::query(db, "due_at IS NOT NULL AND due_at<=? ORDER BY due_at, mastery", &[&t]),
    }
}

// ---------- gems & evolution ----------

pub fn gems(db: &Db) -> i64 {
    db.get_or("gems", 0)
}

pub fn add_gems(db: &Db, n: i64, ev: &mut Events) {
    db.set("gems", &(gems(db) + n));
    ev.gems += n;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Stage {
    pub key: &'static str,
    pub name: &'static str,
    pub next_name: Option<&'static str>,
    pub next_level: Option<i64>,
}

const STAGES: [(i64, &str, &str); 4] = [(1, "spark", "Spark"), (5, "glow", "Glow"), (10, "lumen", "Lumen"), (20, "sage", "Sage")];

pub fn stage_for_level(level: i64) -> Stage {
    let cur = STAGES.iter().rev().find(|s| level >= s.0).unwrap_or(&STAGES[0]);
    let nxt = STAGES.iter().find(|s| s.0 > level);
    Stage { key: cur.1, name: cur.2, next_name: nxt.map(|n| n.2), next_level: nxt.map(|n| n.0) }
}

// ---------- daily quests ----------

pub const QUEST_POOL: &[(&str, &str, &[i64])] = &[
    ("xp", "Earn {n} XP", &[40, 60, 100]),
    ("correct", "Answer {n} questions correctly", &[10, 15, 25]),
    ("combo", "Get {n} right in a row", &[5, 8, 12]),
    ("sessions", "Finish {n} practice sessions", &[1, 2, 3]),
    ("lesson_msgs", "Send Lumi {n} messages in lessons", &[3, 6]),
    ("checks", "Answer {n} check questions in a lesson", &[2, 3]),
    ("perfect", "Finish a session with no mistakes", &[1]),
    ("fixed", "Fix {n} mistakes from your notebook", &[2, 3]),
    ("review", "Complete a spaced review", &[1]),
    ("boss", "Defeat a unit boss", &[1]),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quest {
    pub key: String,
    pub target: i64,
    pub text: String,
    #[serde(default)]
    pub value: i64,
}

pub fn todays_quests(db: &Db) -> Vec<Quest> {
    let key = format!("quests_{}", today());
    if let Some(q) = db.get::<Vec<Quest>>(&key) {
        return q;
    }
    let mut rng = rand::rng();
    let has_concepts = db.int("SELECT COUNT(*) FROM concepts", &[]) > 0;
    let unfixed = db.int("SELECT COUNT(*) FROM mistakes WHERE fixed=0", &[]);
    let mut keys: Vec<&str> = vec!["correct", "combo", "sessions", "lesson_msgs", "checks", "perfect"];
    if unfixed >= 2 {
        keys.push("fixed");
    }
    if !due_concepts(db, None).is_empty() {
        keys.push("review");
    }
    if has_concepts {
        keys.push("boss");
    }
    let mut picked = vec!["xp"]; // always one XP quest
    while picked.len() < 3 && !keys.is_empty() {
        let i = rng.random_range(0..keys.len());
        picked.push(keys.remove(i));
    }
    let qs: Vec<Quest> = picked
        .into_iter()
        .map(|k| {
            let (_, text, targets) = QUEST_POOL.iter().find(|q| q.0 == k).unwrap();
            let mut n = targets[rng.random_range(0..targets.len())];
            if k == "fixed" {
                n = n.min(unfixed);
            }
            Quest { key: k.into(), target: n, text: text.replace("{n}", &n.to_string()), value: 0 }
        })
        .collect();
    db.set(&key, &qs);
    qs
}

pub fn quest_value(db: &Db, key: &str) -> i64 {
    if key == "xp" {
        return today_xp(db);
    }
    db.int("SELECT value FROM quest_progress WHERE day=? AND key=?", &[&today(), &key])
}

pub fn quest_bump(db: &Db, key: &str, n: i64) {
    db.exec(
        "INSERT INTO quest_progress(day, key, value) VALUES(?,?,?) ON CONFLICT(day, key) DO UPDATE SET value=value+excluded.value",
        &[&today(), &key, &n],
    );
}

pub fn quest_max(db: &Db, key: &str, n: i64) {
    db.exec(
        "INSERT INTO quest_progress(day, key, value) VALUES(?,?,?) ON CONFLICT(day, key) DO UPDATE SET value=MAX(value, excluded.value)",
        &[&today(), &key, &n],
    );
}

/// Report quests that became complete since the last check.
pub fn quest_check(db: &Db, ev: &mut Events) {
    let k = format!("quests_done_{}", today());
    let mut done: Vec<String> = db.get_or(&k, vec![]);
    for q in todays_quests(db) {
        if !done.contains(&q.key) && quest_value(db, &q.key) >= q.target {
            done.push(q.key.clone());
            ev.quests_completed.push(q.text);
        }
    }
    db.set(&k, &done);
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Chest {
    Locked,
    Ready,
    Claimed,
}

pub fn quests_state(db: &Db) -> (Vec<Quest>, Chest) {
    let qs: Vec<Quest> = todays_quests(db)
        .into_iter()
        .map(|mut q| {
            q.value = quest_value(db, &q.key).min(q.target);
            q
        })
        .collect();
    let all_done = qs.iter().all(|q| q.value >= q.target);
    let claimed = db.get::<String>("chest_claimed").as_deref() == Some(today().as_str());
    (qs, if claimed { Chest::Claimed } else if all_done { Chest::Ready } else { Chest::Locked })
}

pub fn claim_chest(db: &Db) -> Option<Events> {
    if quests_state(db).1 != Chest::Ready {
        return None;
    }
    db.set("chest_claimed", &today());
    let mut ev = award(db, 15, 0, 0);
    let rewards = [15, 20, 20, 25, 25, 30, 40]; // a small surprise every day
    let mut rng = rand::rng();
    let r = rewards[rng.random_range(0..rewards.len())];
    add_gems(db, r, &mut ev);
    ev.chest_gems = r;
    let freezes: i64 = db.get_or("freezes", 0);
    if rng.random_bool(0.15) && freezes < MAX_FREEZES {
        db.set("freezes", &(freezes + 1));
        ev.chest_freeze = true;
    }
    if counter(db, "chests") >= 7 {
        unlock(db, "quest_7", &mut ev);
    }
    Some(ev)
}

// ---------- shop ----------

pub fn owned(db: &Db) -> Vec<String> {
    db.get_or("owned", vec![])
}

/// Buy an item; Err(reason) when not possible.
pub const DOUBLE_XP_MINUTES: i64 = 15;

pub fn double_xp_active(db: &Db) -> bool {
    db.get_or::<i64>("double_xp_until", 0) > chrono::Utc::now().timestamp()
}

pub fn buy(db: &Db, id: &str) -> Result<Events, String> {
    let item = crate::shop::get(id).ok_or("Unknown item.")?;
    let mut own = owned(db);
    if item.kind != "powerup" && own.iter().any(|o| o == id) {
        return Err("You already own this.".into());
    }
    let freezes: i64 = db.get_or("freezes", 0);
    if id == "freeze" && freezes >= MAX_FREEZES {
        return Err(format!("You can hold at most {MAX_FREEZES} streak freezes."));
    }
    // a mystery box only makes sense while there's something left to win
    let prizes: Vec<&crate::shop::Item> = crate::shop::items().iter().filter(|i| i.kind != "powerup" && !own.contains(&i.id)).collect();
    if id == "mystery" && prizes.is_empty() {
        return Err("You already own everything in the shop. Legendary!".into());
    }
    if gems(db) < item.price {
        return Err("Not enough gems yet. Complete daily quests and beat bosses to earn more!".into());
    }
    db.set("gems", &(gems(db) - item.price));
    let mut ev = Events::default();
    match id {
        "freeze" => db.set("freezes", &(freezes + 1)),
        "double_xp" => {
            // buying again while active adds time
            let now = chrono::Utc::now().timestamp();
            let until = db.get_or::<i64>("double_xp_until", 0).max(now) + DOUBLE_XP_MINUTES * 60;
            db.set("double_xp_until", &until);
        }
        "mystery" => {
            use rand::RngExt;
            // pricier items are rarer
            let weights: Vec<f64> = prizes.iter().map(|p| 1.0 / (p.price as f64).sqrt()).collect();
            let mut roll = rand::rng().random_range(0.0..weights.iter().sum::<f64>());
            let mut prize = prizes[prizes.len() - 1];
            for (p, w) in prizes.iter().zip(&weights) {
                if roll < *w {
                    prize = p;
                    break;
                }
                roll -= w;
            }
            own.push(prize.id.clone());
            db.set("owned", &own);
            db.set(&prize.kind, &prize.id);
            ev.unboxed = Some(prize.id.clone());
        }
        _ => {
            own.push(id.into());
            db.set("owned", &own);
            db.set(&item.kind, &id); // equip right away
        }
    }
    unlock(db, "shopper", &mut ev);
    if own.len() >= 10 {
        unlock(db, "collector", &mut ev);
    }
    Ok(ev)
}

// ---------- dashboard numbers ----------

pub struct DayXp {
    pub day: String,
    pub xp: i64,
    pub frozen: bool,
}

pub fn week(db: &Db) -> Vec<DayXp> {
    (0..7)
        .rev()
        .map(|i| {
            let d = (Local::now().date_naive() - Duration::days(i)).format("%Y-%m-%d").to_string();
            let xp = db.int("SELECT xp FROM activity WHERE day=?", &[&d]);
            DayXp { day: d, xp, frozen: false }
        })
        .collect()
}

/// Activity for the last ~16 weeks, starting on a Monday.
pub fn heatmap(db: &Db) -> (NaiveDate, std::collections::HashMap<String, DayXp>) {
    use chrono::Datelike;
    let today = Local::now().date_naive();
    let start = today - Duration::days(7 * 16 + today.weekday().num_days_from_monday() as i64);
    let s = start.format("%Y-%m-%d").to_string();
    let rows = db.query("SELECT day, xp, frozen FROM activity WHERE day>=?", &[&s], |r| {
        Ok(DayXp { day: r.get(0)?, xp: r.get::<_, Option<i64>>(1)?.unwrap_or(0), frozen: r.get::<_, Option<i64>>(2)?.unwrap_or(0) != 0 })
    });
    (start, rows.into_iter().map(|d| (d.day.clone(), d)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels() {
        assert_eq!(level_for_xp(0), (1, 0, 100));
        assert_eq!(level_for_xp(256).0, 2);
        assert_eq!(level_for_xp(1000).0, 5);
        assert_eq!(stage_for_level(5).key, "glow");
        assert_eq!(mastery_level(59.9), 2);
    }

    fn temp_db(name: &str) -> crate::db::Shared {
        let dir = std::env::temp_dir().join(format!("lumi-game-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Db::open_at(&dir).unwrap()
    }

    #[test]
    fn double_xp_doubles_awards() {
        let db = temp_db("dxp");
        db.set("gems", &100);
        assert_eq!(award(&db, 10, 1, 1).xp, 10);
        buy(&db, "double_xp").unwrap();
        assert_eq!(gems(&db), 100 - crate::shop::get("double_xp").unwrap().price);
        assert_eq!(award(&db, 10, 1, 1).xp, 20);
        // buying again stacks time instead of resetting it
        let first: i64 = db.get_or("double_xp_until", 0);
        buy(&db, "double_xp").unwrap();
        assert_eq!(db.get_or::<i64>("double_xp_until", 0), first + DOUBLE_XP_MINUTES * 60);
    }

    #[test]
    fn mystery_box_gives_new_items_until_none_left() {
        let db = temp_db("mystery");
        let cosmetics: Vec<_> = crate::shop::items().iter().filter(|i| i.kind != "powerup").collect();
        db.set("gems", &100_000);
        for _ in 0..cosmetics.len() {
            let ev = buy(&db, "mystery").unwrap();
            let won = crate::shop::get(ev.unboxed.as_deref().unwrap()).unwrap();
            assert_eq!(db.get::<String>(&won.kind).as_deref(), Some(won.id.as_str()), "the prize is equipped");
        }
        let mut own = owned(&db);
        own.sort();
        own.dedup();
        assert_eq!(own.len(), cosmetics.len(), "never a duplicate");
        let before = gems(&db);
        assert!(buy(&db, "mystery").is_err());
        assert_eq!(gems(&db), before, "no charge when there's nothing left to win");
        assert!(db.int("SELECT COUNT(*) FROM achievements WHERE key='collector'", &[]) == 1);
    }
}
