//! Practice sessions: six question types, combos, hearts, boss battles, written answers graded by AI,
//! missed questions coming back (successive relearning), and the results screen.

use super::md;
use super::widgets::{self, bold, BossMood, Mood};
use super::{App, Confirm, Dialog, Msg, Route};
use crate::actions::{self, Answer, Finish, FinishOut};
use crate::game::Events;
use crate::model::{Concept, Course, Mistake, Test};
use crate::{ai, game, testprep};
use egui::{vec2, Align, Align2, Color32, Key, Layout, RichText, Sense, Stroke, Ui};
use rand::seq::SliceRandom;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Default)]
pub struct Request {
    pub course_id: Option<i64>,
    pub concept_ids: Vec<i64>,
    pub count: usize,
    pub review: bool,
    pub mode: String,         // practice | challenge | boss | lightning
    pub kind: Option<String>, // diagnostic | practice | mock | mistakes | boss
    pub test_id: Option<i64>,
    pub boss: Option<(String, Color32, i64)>, // name, color, unit_idx
    pub return_to: Option<Route>,
}

#[derive(Clone, Default)]
pub struct Data {
    pub course_id: i64,
    pub concepts: Vec<Concept>,
    pub questions: Vec<Value>,
    pub via: String,
}

struct Item {
    q: Value,
    retry: bool,
    tries: u32,
}

#[derive(Default)]
struct QState {
    selected: Option<usize>,
    input: String,
    hint: bool,
    left: Vec<usize>,
    right: Vec<usize>,
    sel: Option<(bool, usize)>,
    matched: HashSet<usize>,
    mistake: bool,
    bad: Option<(usize, usize, f64)>,
    bank: Vec<usize>,
    line: Vec<usize>,
}

struct Feedback {
    ok: bool,
    partial: bool,
    praise: &'static str,
    correct_text: String,
    typo: Option<String>,
    grade: Option<(String, String, f64)>, // feedback, via, score
    user_text: String,
    explain: Option<String>,
    explain_loading: bool,
}

enum Phase {
    Answer,
    Grading,
    Feedback(Feedback),
    OutOfHearts,
    Results,
}

pub struct Run {
    req: Request,
    data: Data,
    queue: VecDeque<Item>,
    cur: Option<Item>,
    q: QState,
    phase: Phase,
    total: usize,
    results: HashMap<i64, Vec<bool>>,
    solved: usize,
    combo: i64,
    best: i64,
    hearts: i64,
    xp: i64,
    first_right: usize,
    t0: f64,
    deferred: Events,
    boss_hp: i64,
    boss_max: i64,
    boss_hit: f64,
    hurt: f64,
    req_id: u64,
    out: Option<FinishOut>,
    before: HashMap<i64, f64>,
    /// Lightning Round: when the clock runs out, and the best score before this round if it was beaten.
    deadline: f64,
    record: Option<i64>,
    t_end: f64,
    /// Set by buttons while the screen is being drawn; acted on once the frame is done,
    /// because closing mid-frame would leave the rest of the screen without a quiz to draw.
    exit: Option<Exit>,
}

enum Exit {
    Close,
    Restart,
}

impl Run {
    fn lightning(&self) -> bool {
        self.req.mode == "lightning"
    }
}

const PRAISE: [&str; 8] = ["Nice!", "Excellent!", "You got it!", "Brilliant!", "Spot on!", "Great job!", "Nailed it!", "Perfect!"];
const COMFORT: [&str; 4] = ["Not quite", "Almost!", "Good try", "So close"];

fn pick<T: Copy>(a: &[T]) -> T {
    use rand::RngExt;
    a[rand::rng().random_range(0..a.len())]
}

// ---------- starting ----------

pub fn start(app: &mut App, req: Request) {
    if req.mode == "lightning" {
        // offline: questions come from the bank, no AI needed
        let r = game::lightning_questions(&app.db, req.course_id)
            .map(|(course_id, questions)| Data { course_id, concepts: Vec::new(), questions, via: "Your question bank".into() })
            .ok_or_else(|| "Lightning Rounds use questions you've already answered. Finish a practice session or two first!".to_string());
        on_ready(app, req, r);
        return;
    }
    if !app.dash.has_key {
        app.error("Add an AI key in Settings first.");
        return;
    }
    let title = match req.kind.as_deref() {
        Some("mistakes") => "Lumi is rewriting your mistakes as fresh questions",
        Some("boss") => "A boss approaches…",
        Some("diagnostic") => "Lumi is writing a quick check",
        Some("mock") => "Lumi is building your mock exam",
        Some("practice") => "Lumi is writing test-style practice",
        _ if req.review => "Gathering your review questions",
        _ => "Lumi is writing your practice session",
    };
    app.busy(title);
    let r2 = req.clone();
    app.spawn(move |db, tx| {
        let r = generate(db, &r2);
        let _ = tx.send(Msg::Quiz(Box::new(r2), r));
    });
}

fn generate(db: &crate::db::Db, req: &Request) -> Result<Data, String> {
    let free = db.mode() == "free";
    let count = if req.count == 0 { 8 } else { req.count }.clamp(3, 20);
    if req.kind.as_deref() == Some("mistakes") {
        let rows = Mistake::unfixed(db, req.course_id, count.min(10) as i64);
        let Some(first) = rows.first() else { return Err("Your mistake notebook is empty. Nothing to fix!".into()) };
        let cid = first.course_id;
        let rows: Vec<Mistake> = rows.into_iter().filter(|m| m.course_id == cid).collect();
        let course = Course::get(db, cid).ok_or("course not found")?;
        let (questions, via) = ai::generate_variants(db, &course, &rows).map_err(|e| e.friendly(free))?;
        let ids: Vec<i64> = rows.iter().filter_map(|m| m.concept_id).collect();
        return Ok(Data { course_id: cid, concepts: Concept::many(db, &ids), questions, via: crate::llm::label(via).into() });
    }
    let test = req.test_id.and_then(|t| Test::get(db, t));
    let mut concepts: Vec<Concept> = if let (Some(t), Some("mock")) = (&test, req.kind.as_deref()) {
        let rows = testprep::concepts(db, t.id);
        if rows.is_empty() {
            return Err("Analyze the test first.".into());
        }
        rows.iter()
            .filter_map(|r| Concept::get(db, r.concept_id).map(|mut c| { c.summary += &format!(" [test priority: {}]", r.priority); c }))
            .collect()
    } else if req.review {
        let due: Vec<Concept> = game::due_concepts(db, req.course_id).into_iter().take(6).collect();
        if due.is_empty() {
            return Err("Nothing is due for review right now. Nice work!".into());
        }
        due
    } else if !req.concept_ids.is_empty() {
        Concept::many(db, &req.concept_ids)
    } else {
        let c = req.course_id.unwrap_or(0);
        Concept::query(db, "course_id=? ORDER BY mastery, unit_idx, idx LIMIT 5", &[&c])
    };
    let Some(first) = concepts.first() else { return Err("Generate a learning path first so Lumi knows what to quiz you on.".into()) };
    let cid = first.course_id;
    concepts.retain(|c| c.course_id == cid); // one course's materials per session
    let course = Course::get(db, cid).ok_or("course not found")?;
    let mids = test.as_ref().map(|t| t.material_ids.clone());
    let (questions, via) = ai::generate_quiz(db, &course, &concepts, count, req.review, test.as_ref(), req.kind.as_deref().filter(|k| *k != "boss"), mids).map_err(|e| e.friendly(free))?;
    Ok(Data { course_id: cid, concepts, questions, via: crate::llm::label(via).into() })
}

pub fn on_ready(app: &mut App, req: Request, r: Result<Data, String>) {
    app.busy = None;
    match r {
        Err(e) => app.error(&e),
        Ok(data) => {
            let total = data.questions.len();
            let before = data.concepts.iter().map(|c| (c.id, c.mastery)).collect();
            let queue: VecDeque<Item> = data.questions.iter().cloned().map(|q| Item { q, retry: false, tries: 0 }).collect();
            let boss_max = if req.mode == "boss" { total as i64 * 12 } else { 0 };
            let t0 = app.ctx.input(|i| i.time);
            let mut run = Run {
                req, data, queue, cur: None, q: QState::default(), phase: Phase::Answer, total, results: HashMap::new(), solved: 0,
                combo: 0, best: 0, hearts: 3, xp: 0, first_right: 0, t0, deferred: Events::default(), boss_hp: boss_max, boss_max,
                boss_hit: -10.0, hurt: -10.0, req_id: 0, out: None, before, deadline: t0 + game::LIGHTNING_SECONDS, record: None, t_end: 0.0, exit: None,
            };
            next(&mut run);
            app.quiz = Some(run);
        }
    }
}

fn next(run: &mut Run) {
    run.cur = run.queue.pop_front();
    run.q = QState::default();
    run.phase = Phase::Answer;
    if let Some(item) = &run.cur {
        let mut rng = rand::rng();
        match item.q["type"].as_str() {
            Some("match_pairs") => {
                let n = item.q["pairs"].as_array().map(|a| a.len()).unwrap_or(0);
                run.q.left = (0..n).collect();
                run.q.right = (0..n).collect();
                run.q.left.shuffle(&mut rng);
                run.q.right.shuffle(&mut rng);
            }
            Some("order_steps") => {
                let n = item.q["ordered_items"].as_array().map(|a| a.len()).unwrap_or(0);
                run.q.bank = (0..n).collect();
                run.q.bank.shuffle(&mut rng);
                if n > 1 && run.q.bank.iter().enumerate().all(|(i, x)| i == *x) {
                    run.q.bank.rotate_left(1);
                }
            }
            _ => {}
        }
    }
}

// ---------- grading helpers ----------

fn norm(s: &str) -> String {
    let s: String = s.to_lowercase().replace('$', "").chars().filter(|c| !"“”\"'`.,;:!?()".contains(*c)).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    for a in ["the ", "a ", "an "] {
        if let Some(r) = s.strip_prefix(a) {
            return r.to_string();
        }
    }
    s
}

fn lev(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + if a[i - 1] == b[j - 1] { 0 } else { 1 });
        }
        prev = cur;
    }
    prev[b.len()]
}

/// (correct, typo-corrected answer)
pub fn grade_blank(input: &str, accepted: &[String]) -> (bool, Option<String>) {
    let u = norm(input);
    if u.is_empty() {
        return (false, None);
    }
    for a in accepted {
        let n = norm(a);
        if u == n {
            return (true, None);
        }
        if let (Ok(x), Ok(y)) = (u.replace(',', "").parse::<f64>(), n.replace(',', "").parse::<f64>()) {
            if (x - y).abs() <= y.abs() * 0.005 + 1e-9 {
                return (true, None);
            }
        }
        if n.chars().count() >= 5 && lev(&u, &n) <= 1 {
            return (true, Some(a.clone()));
        }
    }
    (false, None)
}

fn strs(v: &Value, k: &str) -> Vec<String> {
    v[k].as_array().into_iter().flatten().map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect()
}

pub fn answer_text(q: &Value) -> String {
    match q["type"].as_str() {
        Some("multiple_choice" | "true_false") => strs(q, "options").get(q["correct_index"].as_u64().unwrap_or(0) as usize).cloned().unwrap_or_default(),
        Some("fill_blank") => strs(q, "accepted_answers").first().cloned().unwrap_or_default(),
        Some("order_steps") => strs(q, "ordered_items").iter().enumerate().map(|(i, t)| format!("{}. {t}", i + 1)).collect::<Vec<_>>().join("  "),
        Some("match_pairs") => q["pairs"].as_array().into_iter().flatten().map(|p| format!("{} → {}", p["left"].as_str().unwrap_or(""), p["right"].as_str().unwrap_or(""))).collect::<Vec<_>>().join(" · "),
        Some("short_answer") => q["model_answer"].as_str().unwrap_or("").to_string(),
        _ => String::new(),
    }
}

// ---------- checking ----------

fn check(app: &mut App, skipped: bool) {
    let Some(run) = app.quiz.as_mut() else { return };
    let Some(item) = &run.cur else { return };
    let q = item.q.clone();
    let t = q["type"].as_str().unwrap_or("").to_string();
    if !skipped && t == "short_answer" {
        run.phase = Phase::Grading;
        run.req_id += 1;
        let (id, answer) = (run.req_id, run.q.input.clone());
        app.spawn(move |db, tx| {
            let r = ai::grade(db, &q, &answer).map(|(v, via)| (v, crate::llm::label(via).to_string())).map_err(|e| e.friendly(db.mode() == "free"));
            let _ = tx.send(Msg::Graded(id, r));
        });
        return;
    }
    let (mut ok, mut typo, mut user_text) = (false, None, "(skipped)".to_string());
    if !skipped {
        match t.as_str() {
            "multiple_choice" | "true_false" => {
                let sel = run.q.selected.unwrap_or(usize::MAX);
                ok = sel as u64 == q["correct_index"].as_u64().unwrap_or(99);
                user_text = strs(&q, "options").get(sel).cloned().unwrap_or_default();
            }
            "fill_blank" => {
                let (o, ty) = grade_blank(&run.q.input, &strs(&q, "accepted_answers"));
                ok = o;
                typo = ty;
                user_text = run.q.input.clone();
            }
            "match_pairs" => {
                ok = !run.q.mistake;
                user_text = if run.q.mistake { "made a mismatch".into() } else { "all matched".into() };
            }
            "order_steps" => {
                ok = run.q.line.iter().enumerate().all(|(i, x)| i == *x);
                let items = strs(&q, "ordered_items");
                user_text = run.q.line.iter().map(|i| items.get(*i).cloned().unwrap_or_default()).collect::<Vec<_>>().join(" → ");
            }
            _ => {}
        }
    }
    apply_result(app, ok, false, typo, user_text, None);
    // Lightning Round: keep the pace up; wrong answers still show the right one
    if ok && app.quiz.as_ref().is_some_and(|r| r.lightning()) {
        advance(app);
    }
}

fn apply_result(app: &mut App, ok: bool, partial: bool, typo: Option<String>, user_text: String, grade: Option<(String, String, f64)>) {
    let now = app.ctx.input(|i| i.time);
    let Some(run) = app.quiz.as_mut() else { return };
    let Some(item) = &run.cur else { return };
    let q = item.q.clone();
    let retry = item.retry;
    let tries = item.tries;
    let concept = q["concept_id"].as_i64().unwrap_or(0);
    let exam = run.data.questions.is_empty() || run.req.kind.as_deref() == Some("mock") || run.req.mode == "boss" || run.lightning();
    if !retry {
        run.results.entry(concept).or_default().push(ok);
        if ok {
            run.first_right += 1;
        }
    }
    if ok {
        run.solved += 1;
        if !retry {
            run.combo += 1;
        }
        run.best = run.best.max(run.combo);
    } else {
        run.combo = 0;
        if !exam && tries < 2 {
            run.queue.push_back(Item { q: q.clone(), retry: true, tries: tries + 1 }); // successive relearning: it comes back
        } else {
            run.solved += 1;
        }
        if run.req.mode == "challenge" || run.req.mode == "boss" {
            run.hearts -= 1;
        }
    }
    if run.req.mode == "boss" {
        if ok {
            let dmg = 10 + (2 * (run.combo - 1).max(0)).min(10);
            run.boss_hp = (run.boss_hp - dmg).max(0);
            run.boss_hit = now;
        } else {
            run.hurt = now;
        }
    }
    let combo = run.combo;
    run.phase = Phase::Feedback(Feedback {
        ok, partial, praise: if ok { pick(&PRAISE) } else { pick(&COMFORT) }, correct_text: answer_text(&q), typo, grade,
        user_text: user_text.clone(), explain: None, explain_loading: false,
    });
    let course_id = run.data.course_id;
    let mistake_id = q["mistake_id"].as_i64();
    if ok {
        if combo >= 3 { app.sound.play_n("combo", combo) } else { app.sound.play("correct") }
    } else {
        app.sound.play("wrong");
    }
    let ev = actions::answer(&app.db, &Answer {
        concept_id: Some(concept).filter(|c| *c > 0), course_id: Some(course_id), correct: ok, combo, retry,
        question: Some(&q), user_answer: &user_text, mistake_id, from_check_card: false,
    });
    let run = app.quiz.as_mut().unwrap();
    run.xp += ev.xp;
    // big celebrations wait for the results screen
    run.deferred.level_up = ev.level_up.or(run.deferred.level_up);
    run.deferred.goal_hit |= ev.goal_hit;
    run.deferred.evolved = ev.evolved.clone().or(run.deferred.evolved.take());
    let at = app.ctx.content_rect().center_bottom() - vec2(0.0, 170.0);
    app.float_xp(ev.xp, at);
    app.handle(&ev, true);
}

pub fn on_graded(app: &mut App, id: u64, r: Result<(Value, String), String>) {
    let Some(run) = app.quiz.as_mut() else { return };
    if run.req_id != id || !matches!(run.phase, Phase::Grading) {
        return;
    }
    match r {
        Err(e) => {
            run.phase = Phase::Answer; // let them try again
            app.error(&e);
        }
        Ok((g, via)) => {
            let verdict = g["verdict"].as_str().unwrap_or("incorrect").to_string();
            let answer = run.q.input.clone();
            apply_result(app, verdict == "correct", verdict == "partial", None, answer,
                Some((g["feedback"].as_str().unwrap_or("").to_string(), via, g["score"].as_f64().unwrap_or(0.0))));
        }
    }
}

pub fn on_explain(app: &mut App, id: u64, chunk: Option<String>, done: bool) {
    let Some(run) = app.quiz.as_mut() else { return };
    if run.req_id != id {
        return;
    }
    if let Phase::Feedback(fb) = &mut run.phase {
        match chunk {
            Some(c) => fb.explain.get_or_insert_with(String::new).push_str(&c),
            None if !done => fb.explain = Some(String::new()),
            None => {}
        }
        if done {
            fb.explain_loading = false;
        }
    }
}

fn advance(app: &mut App) {
    let Some(run) = app.quiz.as_mut() else { return };
    if run.req.mode == "boss" && (run.boss_hp <= 0 || run.hearts <= 0) {
        return finish(app);
    }
    if run.req.mode == "challenge" && run.hearts <= 0 {
        run.phase = Phase::OutOfHearts;
        return;
    }
    if run.queue.is_empty() {
        return finish(app);
    }
    next(run);
}

fn finish(app: &mut App) {
    let now = app.ctx.input(|i| i.time);
    let Some(run) = app.quiz.as_mut() else { return };
    run.t_end = now;
    if run.lightning() {
        run.total = run.results.values().map(|v| v.len()).sum(); // questions actually answered
    }
    let won = run.req.mode == "boss" && run.boss_hp <= 0;
    let boss = run.req.boss.as_ref().map(|b| (b.2, won));
    let out = actions::finish(&app.db, &Finish {
        course_id: run.data.course_id, results: &run.results, seconds: (now - run.t0) as i64, mode: &run.req.mode,
        kind: run.req.kind.as_deref(), review: run.req.review, hearts_left: run.hearts, test_id: run.req.test_id, boss,
    });
    let mut ev = out.events.clone();
    if run.lightning() {
        run.record = game::lightning_score(&app.db, run.first_right as i64, &mut ev);
    }
    ev.level_up = ev.level_up.or(run.deferred.level_up);
    ev.goal_hit |= run.deferred.goal_hit;
    ev.evolved = ev.evolved.take().or(run.deferred.evolved.take());
    run.deferred = Events::default();
    let lost = run.req.mode == "boss" && !won;
    run.out = Some(out);
    run.phase = Phase::Results;
    if lost {
        app.sound.play("wrong");
    } else {
        app.sound.play("complete");
        let perfect = app.quiz.as_ref().and_then(|r| r.out.as_ref()).map(|o| o.perfect).unwrap_or(false);
        app.confetti(if perfect || won { 260 } else { 150 });
    }
    app.handle(&ev, false);
}

pub fn close(app: &mut App) {
    let Some(run) = app.quiz.take() else { return };
    // quit early: still celebrate what happened
    if run.out.is_none() && (run.deferred.level_up.is_some() || run.deferred.goal_hit || run.deferred.evolved.is_some()) {
        app.handle(&run.deferred, false);
    }
    match run.req.return_to {
        Some(r) => app.go(r),
        None => {
            let r = app.route.clone();
            app.go(r);
        }
    }
}

// ---------- drawing ----------

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let now = ui.input(|i| i.time);
    if app.quiz.as_ref().is_some_and(|r| r.lightning() && !matches!(r.phase, Phase::Results)) {
        if now >= app.quiz.as_ref().unwrap().deadline {
            finish(app);
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    let Some(run) = app.quiz.as_ref() else { return };
    let hurt = now - run.hurt < 0.4;
    let shake = if hurt { ((now * 60.0).sin() * 8.0) as f32 } else { 0.0 };
    let width = ui.available_width().min(960.0);
    let pad = (ui.available_width() - width) / 2.0;
    // top bar
    egui::Panel::top("quiz_top").frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 22, bottom: 8 })).show(ui, |ui| {
        ui.add_space(0.0);
        let _ = pad;
        super::column(ui, width, |ui| {
        ui.add_space(0.0);
        if shake != 0.0 { ui.add_space(shake.abs()); }
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("✕").size(24.0).color(p.faint)).frame(false)).on_hover_text("Quit").clicked() {
                let run = app.quiz.as_ref().unwrap();
                if run.solved == 0 || matches!(run.phase, Phase::Results) {
                    app.quiz.as_mut().unwrap().exit = Some(Exit::Close);
                } else {
                    app.dialog = Some(Dialog::Confirm { title: "Wait, don't go!".into(), body: "You'll lose this session's completion bonus if you quit now.".into(), action: Confirm::QuitQuiz });
                }
                return;
            }
            let run = app.quiz.as_ref().unwrap();
            let right = 190.0;
            if run.lightning() {
                // the bar is the clock
                let left = if matches!(run.phase, Phase::Results) { 0.0 } else { (run.deadline - now).max(0.0) };
                let color = if left < 10.0 { p.red } else if left < 25.0 { p.gold } else { p.primary2 };
                widgets::progress(ui, (left / game::LIGHTNING_SECONDS) as f32, color, 16.0, (ui.available_width() - right - 60.0).max(100.0));
                ui.label(RichText::new(format!("⏱ {}", left.ceil() as i64)).font(bold(18.0)).color(color));
                ui.label(RichText::new(format!("⚡ {}", run.first_right)).font(bold(18.0)).color(p.gold)).on_hover_text("Correct answers this round");
            } else {
                let frac = if matches!(run.phase, Phase::Results) { 1.0 } else { run.solved as f32 / run.total.max(1) as f32 };
                widgets::progress(ui, frac, p.green, 16.0, (ui.available_width() - right).max(100.0));
            }
            ui.label(RichText::new(format!("✍️ {}", run.data.via)).size(11.0).color(p.faint)).on_hover_text("The AI that wrote these questions");
            if run.combo >= 2 {
                ui.label(RichText::new(format!("🔥 {}", run.combo)).font(bold(18.0)).color(p.flame));
            }
            if run.req.mode == "challenge" || run.req.mode == "boss" {
                ui.label(RichText::new(format!("{}{}", "❤️".repeat(run.hearts.max(0) as usize), "🖤".repeat((3 - run.hearts).clamp(0, 3) as usize))).size(18.0));
            }
        });
        let run = app.quiz.as_ref().unwrap();
        if let Some((name, color, _)) = &run.req.boss {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let hit = now - run.boss_hit < 0.6;
                let mood = if run.boss_hp <= 0 { BossMood::Defeated } else if hit { BossMood::Hurt } else { BossMood::Idle };
                let jiggle = if hit { ((now * 50.0).sin() * 6.0) as f32 } else { 0.0 };
                let (r, _) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::hover());
                widgets::boss(ui, r.translate(vec2(jiggle, 0.0)), *color, mood);
                if hit {
                    let dmg = 10 + (2 * (run.combo - 1).max(0)).min(10);
                    ui.painter().text(r.right_top() + vec2(10.0, -(now - run.boss_hit) as f32 * 60.0), Align2::LEFT_TOP, format!("-{dmg}{}", if run.combo >= 3 { " CRIT!" } else { "" }), bold(22.0), p.red);
                    ui.ctx().request_repaint();
                }
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("⚔️ {name}")).font(bold(15.0)));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(format!("{} / {} HP", run.boss_hp, run.boss_max)).size(13.0)));
                    });
                    widgets::progress(ui, run.boss_hp as f32 / run.boss_max.max(1) as f32, p.red, 14.0, ui.available_width());
                });
            });
        }
        });
    });
    if hurt {
        ui.ctx().request_repaint();
    }
    egui::Panel::bottom("quiz_foot").frame(egui::Frame::new().fill(foot_color(app)).inner_margin(egui::Margin { left: 0, right: 0, top: 18, bottom: 22 })).show(ui, |ui| {
        super::column(ui, width, |ui| footer(app, ui));
    });
    egui::CentralPanel::no_frame().show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            super::column(ui, 720.0, |ui| {
                ui.add_space(10.0);
                body(app, ui);
            });
        });
    });
    // keyboard
    let enter = ui.input(|i| i.key_pressed(Key::Enter) && (i.modifiers.ctrl || i.modifiers.command || !matches!(app.quiz.as_ref().and_then(|r| r.cur.as_ref()).map(|c| c.q["type"].as_str() == Some("short_answer")), Some(true)) || !matches!(app.quiz.as_ref().map(|r| &r.phase), Some(Phase::Answer))));
    if enter && app.dialog.is_none() {
        match app.quiz.as_ref().map(|r| &r.phase) {
            Some(Phase::Answer) if can_check(app) => check(app, false),
            Some(Phase::Feedback(_)) => advance(app),
            Some(Phase::Results) => app.quiz.as_mut().unwrap().exit = Some(Exit::Close),
            _ => {}
        }
    }
    if let Some(run) = app.quiz.as_mut() {
        if matches!(run.phase, Phase::Answer) {
            if let Some(item) = &run.cur {
                if matches!(item.q["type"].as_str(), Some("multiple_choice" | "true_false")) {
                    let n = item.q["options"].as_array().map(|a| a.len()).unwrap_or(0);
                    for (i, k) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5].iter().enumerate() {
                        if i < n && ui.input(|inp| inp.key_pressed(*k)) {
                            run.q.selected = Some(i);
                        }
                    }
                }
            }
        }
    }
    // the whole screen is drawn: now it's safe to leave
    match app.quiz.as_mut().and_then(|r| r.exit.take()) {
        Some(Exit::Close) => close(app),
        Some(Exit::Restart) => {
            let req = app.quiz.take().unwrap().req;
            start(app, req);
        }
        None => {}
    }
}

fn foot_color(app: &App) -> Color32 {
    let p = app.pal;
    match app.quiz.as_ref().map(|r| &r.phase) {
        Some(Phase::Feedback(fb)) if fb.ok => widgets::lerp(p.bg, p.green, 0.14),
        Some(Phase::Feedback(fb)) if fb.partial => widgets::lerp(p.bg, p.gold, 0.13),
        Some(Phase::Feedback(_)) => widgets::lerp(p.bg, p.red, 0.13),
        _ => p.bg,
    }
}

fn can_check(app: &App) -> bool {
    let Some(run) = app.quiz.as_ref() else { return false };
    let Some(item) = &run.cur else { return false };
    match item.q["type"].as_str() {
        Some("multiple_choice" | "true_false") => run.q.selected.is_some(),
        Some("fill_blank" | "short_answer") => !run.q.input.trim().is_empty(),
        Some("match_pairs") => run.q.matched.len() == run.q.left.len(),
        Some("order_steps") => run.q.bank.is_empty() && !run.q.line.is_empty(),
        _ => false,
    }
}

const TYPE_LABEL: &[(&str, &str)] = &[("short_answer", "Answer in your own words"), ("multiple_choice", "Select the correct answer"), ("true_false", "True or false?"),
    ("fill_blank", "Fill in the blank"), ("match_pairs", "Tap the matching pairs"), ("order_steps", "Put these in the right order")];

fn kind_label(k: Option<&str>) -> Option<&'static str> {
    Some(match k? {
        "diagnostic" => "⚡ QUICK CHECK · DO YOU ALREADY KNOW THIS?",
        "practice" => "🎯 TEST PRACTICE",
        "mock" => "📝 MOCK EXAM",
        "mistakes" => "📓 FIXING A PAST MISTAKE",
        "boss" => "⚔️ BOSS BATTLE",
        _ => return None,
    })
}

fn body(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let run = app.quiz.as_ref().unwrap();
    match &run.phase {
        Phase::Results => return results(app, ui),
        Phase::OutOfHearts => {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                app.mascot(ui, 160.0, Mood::Sad);
                ui.label(RichText::new("Out of hearts 💔").font(bold(30.0)));
                ui.label(RichText::new("That was a tough one. No problem: keep going in practice mode and finish the session.").color(p.muted));
            });
            return;
        }
        _ => {}
    }
    let Some(item) = &run.cur else { return };
    let q = item.q.clone();
    let retry = item.retry;
    let t = q["type"].as_str().unwrap_or("");
    let fb_ok = if let Phase::Feedback(fb) = &run.phase { Some(fb.ok) } else { None };
    if retry {
        ui.label(RichText::new("↻ LET'S TRY THIS ONE AGAIN").font(bold(13.0)).color(p.flame));
    } else if let Some(k) = kind_label(run.req.kind.as_deref()) {
        ui.label(RichText::new(k).font(bold(13.0)).color(p.primary2));
    }
    ui.label(RichText::new(TYPE_LABEL.iter().find(|x| x.0 == t).map(|x| x.1).unwrap_or("Question")).font(bold(26.0)));
    ui.add_space(10.0);
    ui.horizontal_top(|ui| {
        app.mascot(ui, 70.0, match fb_ok { Some(true) => Mood::Happy, Some(false) => Mood::Sad, None => Mood::Idle });
        egui::Frame::new().stroke(Stroke::new(2.0, p.line)).corner_radius(18).inner_margin(16).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.scope(|ui| {
                ui.style_mut().override_text_style = Some(egui::TextStyle::Body);
                md::show(ui, &mut app.md_cache, q["prompt"].as_str().unwrap_or(""));
            });
        });
    });
    ui.add_space(18.0);
    let answering = matches!(app.quiz.as_ref().unwrap().phase, Phase::Answer);
    match t {
        "multiple_choice" | "true_false" => options(app, ui, &q, answering),
        "fill_blank" => {
            let run = app.quiz.as_mut().unwrap();
            let r = ui.add_enabled(answering, egui::TextEdit::singleline(&mut run.q.input).hint_text("Type your answer").font(egui::FontId::proportional(20.0)).desired_width(f32::INFINITY).margin(vec2(16.0, 14.0)));
            if answering && !r.has_focus() && run.q.input.is_empty() {
                r.request_focus();
            }
            hint(app, ui, &q);
        }
        "short_answer" => {
            let grading = matches!(app.quiz.as_ref().unwrap().phase, Phase::Grading);
            let run = app.quiz.as_mut().unwrap();
            if answering || grading {
                ui.add_enabled(answering, egui::TextEdit::multiline(&mut run.q.input).hint_text("Write your answer in your own words: explain your reasoning or show your steps.").desired_rows(5).desired_width(f32::INFINITY).font(egui::FontId::proportional(16.0)));
                ui.label(RichText::new("Graded by AI with partial credit · Ctrl+Enter to check").size(12.0).color(p.faint));
                hint(app, ui, &q);
            } else {
                widgets::card(&p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Your answer").font(bold(13.0)).color(p.muted));
                    ui.label(&run.q.input);
                });
            }
        }
        "match_pairs" => match_pairs(app, ui, &q, answering),
        "order_steps" => order_steps(app, ui, &q, answering),
        _ => {}
    }
}

fn hint(app: &mut App, ui: &mut Ui, q: &Value) {
    let p = app.pal;
    let h = q["hint"].as_str().unwrap_or("");
    if h.is_empty() {
        return;
    }
    let run = app.quiz.as_mut().unwrap();
    if run.q.hint {
        egui::Frame::new().fill(widgets::lerp(p.bg, p.gold, 0.1)).corner_radius(12).inner_margin(10).show(ui, |ui| ui.label(RichText::new(md::math_to_unicode(h)).color(p.gold)));
    } else if ui.link(RichText::new("💡 Need a hint?").font(bold(14.0)).color(p.gold)).clicked() {
        run.q.hint = true;
    }
}

fn option_tile(ui: &mut Ui, p: &super::theme::Pal, text: &str, key: Option<usize>, state: u8, w: f32, h: f32) -> egui::Response {
    // state: 0 idle, 1 selected, 2 right, 3 wrong
    let (border, fill) = match state {
        1 => (p.primary, widgets::lerp(p.surface, p.primary, 0.15)),
        2 => (p.green, widgets::lerp(p.surface, p.green, 0.15)),
        3 => (p.red, widgets::lerp(p.surface, p.red, 0.15)),
        _ => (p.line, p.surface),
    };
    let galley = ui.painter().layout(md::math_to_unicode(text), egui::FontId::proportional(17.0), p.text, w - 70.0);
    let height = (galley.size().y + 30.0).max(h);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height + 4.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let body = egui::Rect::from_min_size(rect.min, vec2(w, height));
    ui.painter().rect_filled(body.translate(vec2(0.0, 4.0)), 16, widgets::darken(border, 0.7));
    ui.painter().rect(body, 16, if resp.hovered() && state == 0 { p.surface2 } else { fill }, Stroke::new(2.0, border), egui::StrokeKind::Inside);
    let mut x = body.left() + 18.0;
    if let Some(k) = key {
        let kr = egui::Rect::from_min_size(egui::pos2(x, body.center().y - 15.0), vec2(30.0, 30.0));
        ui.painter().rect(kr, 9, Color32::TRANSPARENT, Stroke::new(2.0, if state == 1 { p.primary } else { p.line }), egui::StrokeKind::Inside);
        ui.painter().text(kr.center(), Align2::CENTER_CENTER, (k + 1).to_string(), bold(13.0), p.faint);
        x += 44.0;
    }
    let gp = if key.is_none() && h > 60.0 { body.center() - galley.size() / 2.0 } else { egui::pos2(x, body.center().y - galley.size().y / 2.0) };
    ui.painter().galley(gp, galley, p.text);
    resp
}

fn options(app: &mut App, ui: &mut Ui, q: &Value, answering: bool) {
    let p = app.pal;
    let opts = strs(q, "options");
    let correct = q["correct_index"].as_u64().unwrap_or(99) as usize;
    let tf = q["type"] == "true_false";
    let run = app.quiz.as_mut().unwrap();
    let sel = run.q.selected;
    let mut clicked = None;
    let w = ui.available_width();
    let state = |i: usize| -> u8 {
        if answering { if sel == Some(i) { 1 } else { 0 } } else if i == correct { 2 } else if sel == Some(i) { 3 } else { 0 }
    };
    if tf {
        ui.horizontal(|ui| {
            for (i, o) in opts.iter().enumerate() {
                if option_tile(ui, &p, o, Some(i), state(i), (w - 14.0) / 2.0, 80.0).clicked() && answering {
                    clicked = Some(i);
                }
            }
        });
    } else {
        for (i, o) in opts.iter().enumerate() {
            if option_tile(ui, &p, o, Some(i), state(i), w, 54.0).clicked() && answering {
                clicked = Some(i);
            }
            ui.add_space(6.0);
        }
    }
    if let Some(i) = clicked {
        app.quiz.as_mut().unwrap().q.selected = Some(i);
        app.sound.play("tap");
    }
}

fn match_pairs(app: &mut App, ui: &mut Ui, q: &Value, answering: bool) {
    let p = app.pal;
    let now = ui.input(|i| i.time);
    let pairs: Vec<(String, String)> = q["pairs"].as_array().into_iter().flatten().map(|x| (x["left"].as_str().unwrap_or("").to_string(), x["right"].as_str().unwrap_or("").to_string())).collect();
    let run = app.quiz.as_mut().unwrap();
    let (left, right) = (run.q.left.clone(), run.q.right.clone());
    let w = (ui.available_width() - 14.0) / 2.0;
    let mut clicked: Option<(bool, usize)> = None;
    ui.horizontal_top(|ui| {
        for (is_left, order) in [(true, &left), (false, &right)] {
            ui.vertical(|ui| {
                for &i in order.iter() {
                    let text = if is_left { &pairs[i].0 } else { &pairs[i].1 };
                    let done = run.q.matched.contains(&i);
                    let bad = run.q.bad.map(|(a, b, t)| now - t < 0.45 && ((is_left && a == i) || (!is_left && b == i))).unwrap_or(false);
                    let sel = run.q.sel == Some((is_left, i));
                    let st = if done { 2 } else if bad { 3 } else if sel { 1 } else { 0 };
                    let resp = ui.add_enabled_ui(!done && answering, |ui| option_tile(ui, &p, text, None, st, w, 64.0)).inner;
                    if resp.clicked() && !done && answering {
                        clicked = Some((is_left, i));
                    }
                    ui.add_space(6.0);
                }
            });
        }
    });
    if run.q.bad.map(|b| now - b.2 < 0.5).unwrap_or(false) {
        ui.ctx().request_repaint();
    }
    if let Some((is_left, i)) = clicked {
        match run.q.sel {
            Some((sl, si)) if sl != is_left => {
                let (l, r) = if is_left { (i, si) } else { (si, i) };
                if l == r {
                    run.q.matched.insert(l);
                    app.sound.play("correct");
                } else {
                    run.q.mistake = true;
                    run.q.bad = Some((l, r, now));
                    app.sound.play("wrong");
                }
                app.quiz.as_mut().unwrap().q.sel = None;
            }
            _ => {
                run.q.sel = Some((is_left, i));
                app.sound.play("tap");
            }
        }
    }
}

fn order_steps(app: &mut App, ui: &mut Ui, q: &Value, answering: bool) {
    let p = app.pal;
    let items = strs(q, "ordered_items");
    let run = app.quiz.as_mut().unwrap();
    let w = ui.available_width();
    let mut take_line = None;
    let mut take_bank = None;
    egui::Frame::new().stroke(Stroke::new(2.0, p.line)).inner_margin(egui::Margin::symmetric(0, 10)).show(ui, |ui| {
        ui.set_width(w);
        if run.q.line.is_empty() {
            ui.label(RichText::new("   Tap the steps below in order").color(p.faint));
        }
        for (k, &i) in run.q.line.iter().enumerate() {
            if option_tile(ui, &p, &format!("{}. {}", k + 1, items[i]), None, 0, w, 44.0).clicked() && answering {
                take_line = Some(k);
            }
            ui.add_space(4.0);
        }
    });
    ui.add_space(14.0);
    ui.horizontal_wrapped(|ui| {
        for (k, &i) in run.q.bank.iter().enumerate() {
            let tw = (ui.painter().layout_no_wrap(items[i].clone(), egui::FontId::proportional(17.0), p.text).size().x + 72.0).min(w);
            if option_tile(ui, &p, &items[i], None, 0, tw, 44.0).clicked() && answering {
                take_bank = Some(k);
            }
        }
    });
    if let Some(k) = take_line {
        let x = run.q.line.remove(k);
        run.q.bank.push(x);
        app.sound.play("tap");
    } else if let Some(k) = take_bank {
        let x = run.q.bank.remove(k);
        run.q.line.push(x);
        app.sound.play("tap");
    }
}

fn footer(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let run = app.quiz.as_ref().unwrap();
    match &run.phase {
        Phase::Answer => {
            ui.horizontal(|ui| {
                if widgets::button(ui, "Skip", p.surface2, true).clicked() {
                    check(app, true);
                    return;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let t = app.quiz.as_ref().unwrap().cur.as_ref().map(|c| c.q["type"].as_str() == Some("match_pairs")).unwrap_or(false);
                    if widgets::button_sized(ui, if t { "Continue" } else { "Check" }, p.green, can_check(app), 15.0, 140.0).clicked() {
                        check(app, false);
                    }
                });
            });
        }
        Phase::Grading => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().color(p.primary2));
                ui.label(RichText::new("Grading your answer…").font(bold(16.0)));
            });
        }
        Phase::OutOfHearts => {
            ui.horizontal(|ui| {
                if widgets::button(ui, "End session", p.surface2, true).clicked() {
                    finish(app);
                    return;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button(ui, "Keep practicing", p.primary, true).clicked() {
                        let run = app.quiz.as_mut().unwrap();
                        run.req.mode = "practice".into();
                        advance(app);
                    }
                });
            });
        }
        Phase::Feedback(fb) => {
            let (ok, partial) = (fb.ok, fb.partial);
            let combo = run.combo;
            let mut explain_clicked = false;
            let fb = Feedback { ok, partial, praise: fb.praise, correct_text: fb.correct_text.clone(), typo: fb.typo.clone(), grade: fb.grade.clone(),
                user_text: fb.user_text.clone(), explain: fb.explain.clone(), explain_loading: fb.explain_loading };
            let q = run.cur.as_ref().map(|c| c.q.clone()).unwrap_or_default();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - 180.0);
                    let title = if ok {
                        format!("✅ {}{}", fb.praise, if combo >= 3 { format!("  🔥 {combo} in a row!") } else { String::new() })
                    } else if partial {
                        format!("🟡 Partly right ({}%)", (fb.grade.as_ref().map(|g| g.2).unwrap_or(0.0) * 100.0) as i64)
                    } else {
                        format!("❌ {}", fb.praise)
                    };
                    ui.label(RichText::new(title).font(bold(22.0)).color(if ok { p.green } else if partial { p.gold } else { p.red }));
                    if let Some((feedback, via, _)) = &fb.grade {
                        ui.label(RichText::new(md::math_to_unicode(feedback)).strong());
                        ui.label(RichText::new(format!("Graded by {via}")).size(11.0).color(p.faint));
                    }
                    if let Some(t) = &fb.typo {
                        ui.label(RichText::new(format!("Watch the spelling: {t}")).strong());
                    }
                    if !ok && !fb.correct_text.is_empty() {
                        let label = if fb.grade.is_some() { "Model answer" } else { "Correct answer" };
                        ui.label(RichText::new(format!("{label}: {}", md::math_to_unicode(&fb.correct_text))).strong());
                    }
                    let expl = q["explanation"].as_str().unwrap_or("");
                    if !expl.is_empty() {
                        ui.label(md::math_to_unicode(expl));
                    }
                    match (&fb.explain, fb.explain_loading) {
                        (Some(text), _) if !text.is_empty() => {
                            ui.add_space(4.0);
                            egui::Frame::new().fill(widgets::lerp(p.bg, p.primary, 0.12)).corner_radius(12).inner_margin(10).show(ui, |ui| md::show(ui, &mut app.md_cache, text));
                        }
                        (_, true) => {
                            ui.add(egui::Spinner::new().color(p.primary2));
                        }
                        _ if !ok => {
                            if ui.link(RichText::new("🤔 Explain this to me").font(bold(14.0)).color(p.primary2)).clicked() {
                                explain_clicked = true;
                            }
                        }
                        _ => {}
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button_sized(ui, "Continue", if ok { p.green } else if partial { p.gold } else { p.red }, true, 15.0, 150.0).clicked() {
                        advance(app);
                    }
                });
            });
            if explain_clicked {
                let run = app.quiz.as_mut().unwrap();
                run.req_id += 1;
                let id = run.req_id;
                let q = run.cur.as_ref().map(|c| c.q.clone()).unwrap_or_default();
                let ua = if let Phase::Feedback(fb) = &mut run.phase {
                    fb.explain_loading = true;
                    fb.user_text.clone()
                } else {
                    String::new()
                };
                app.spawn(move |db, tx| {
                    let r = ai::explain(db, &q, &ua, &mut |ev| match ev {
                        crate::llm::Ev::Text(t) => {
                            let _ = tx.send(Msg::Explain(id, Some(t), false));
                        }
                        crate::llm::Ev::Reset => {
                            let _ = tx.send(Msg::Explain(id, None, false));
                        }
                        _ => {}
                    });
                    if let Err(e) = r {
                        let _ = tx.send(Msg::Explain(id, Some(e.friendly(db.mode() == "free")), false));
                    }
                    let _ = tx.send(Msg::Explain(id, None, true));
                });
            }
        }
        Phase::Results => {
            let lost = run.req.mode == "boss" && run.boss_hp > 0;
            let lightning = run.lightning();
            let back = matches!(run.req.return_to, Some(Route::Test(_)));
            ui.horizontal(|ui| {
                let again = if lost { Some("⚔️ Rematch") } else if lightning { Some("⚡ Play again") } else { None };
                if let Some(label) = again {
                    if widgets::button(ui, label, p.surface2, true).clicked() {
                        app.quiz.as_mut().unwrap().exit = Some(Exit::Restart);
                        return;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button_sized(ui, if back { "Back to test plan" } else { "Continue" }, p.green, true, 15.0, 160.0).clicked() {
                        app.quiz.as_mut().unwrap().exit = Some(Exit::Close);
                    }
                });
            });
        }
    }
}

fn results(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let run = app.quiz.as_ref().unwrap();
    let Some(out) = &run.out else { return };
    let acc = if run.total > 0 { 100 * run.first_right / run.total } else { 0 };
    let secs = (run.t_end - run.t0) as i64;
    let boss = run.req.boss.clone();
    let lost = boss.is_some() && run.boss_hp > 0;
    let exam = run.req.kind.as_deref() == Some("mock");
    let (stage, hat, wear) = (app.dash.stage_key.clone(), app.dash.hat.clone(), app.dash.accessory.clone());
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        if let Some((name, color, _)) = &boss {
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 260.0) / 2.0);
                widgets::mascot_widget(ui, 130.0, if lost { Mood::Sad } else { Mood::Celebrate }, &stage, hat.as_deref(), wear.as_deref());
                let (r, _) = ui.allocate_exact_size(vec2(110.0, 110.0), Sense::hover());
                widgets::boss(ui, r, *color, if lost { BossMood::Idle } else { BossMood::Defeated });
            });
            ui.label(RichText::new(if lost { format!("{name} wins this round") } else { format!("Victory! {name} defeated ⚔️") }).font(bold(30.0)));
            ui.label(RichText::new(if lost { format!("It had {} HP left. Review the unit and come back stronger, you've got this.", run.boss_hp) } else { "Unit conquered! The boss drops a pile of gems. 💎".into() }).color(p.muted));
        } else {
            widgets::mascot_widget(ui, 160.0, Mood::Celebrate, &stage, hat.as_deref(), wear.as_deref());
            if run.lightning() {
                ui.label(RichText::new(format!("⚡ {} correct in {}s", run.first_right, game::LIGHTNING_SECONDS as i64)).font(bold(30.0)));
                let best: i64 = app.db.get_or("lightning_best", 0);
                let line = match run.record {
                    Some(prev) if prev > 0 => format!("New high score! You beat your old best of {prev}. 🏆"),
                    Some(_) => "Your first high score. Now beat it! 🏆".into(),
                    None => format!("Your best is {best}. So close. Go again?"),
                };
                ui.label(RichText::new(line).color(if run.record.is_some() { p.gold } else { p.muted }));
            } else {
            let title = if exam { format!("Mock exam: {}%", out.mock_score.unwrap_or(acc as i64)) } else if out.perfect { "Flawless! 💎".into() } else if acc >= 80 { "Lesson complete! 🎉".into() } else { "Practice complete! 💪".into() };
            ui.label(RichText::new(title).font(bold(30.0)));
            ui.label(RichText::new(if out.perfect { "Not a single mistake. Incredible." } else if acc >= 80 { "Strong work. That's real understanding." } else { "Every mistake you fixed today is a mistake you won't make on the test." }).color(p.muted));
            }
        }
        if let Some(m) = &out.test_message {
            ui.add_space(10.0);
            widgets::card_glow(&p).show(ui, |ui| {
                ui.set_max_width(520.0);
                ui.label(m);
            });
        }
        ui.add_space(16.0);
        let tiles = [
            ("TOTAL XP", format!("⚡ {}", run.xp + out.bonus), p.gold),
            ("ACCURACY", format!("🎯 {acc}%"), p.green),
            ("TIME", format!("⏱ {}:{:02}", secs / 60, secs % 60), p.primary2),
        ];
        ui.horizontal(|ui| {
            let n = if run.best >= 3 { 4.0 } else { 3.0 };
            ui.add_space(((ui.available_width() - n * 154.0) / 2.0).max(0.0));
            for (label, val, color) in tiles.iter().chain(if run.best >= 3 { Some(("BEST COMBO", format!("🔥 {}", run.best), p.flame)) } else { None }.iter()) {
                egui::Frame::new().stroke(Stroke::new(3.0, *color)).corner_radius(16).show(ui, |ui| {
                    ui.set_width(140.0);
                    ui.vertical_centered(|ui| {
                        egui::Frame::new().fill(*color).corner_radius(egui::CornerRadius { nw: 13, ne: 13, sw: 0, se: 0 }).inner_margin(6).show(ui, |ui| {
                            ui.set_width(140.0);
                            ui.label(RichText::new(*label).font(bold(12.0)).color(p.bg));
                        });
                        ui.add_space(10.0);
                        ui.label(RichText::new(val).font(bold(22.0)));
                        ui.add_space(10.0);
                    });
                });
            }
        });
        ui.add_space(16.0);
        let concepts = out.concepts.clone();
        let before = run.before.clone();
        for c in concepts {
            ui.allocate_ui(vec2(420.0, 40.0), |ui| {
                ui.set_width(420.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&c.title).font(bold(14.0)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let b = before.get(&c.id).copied().unwrap_or(0.0);
                        if c.mastery > b {
                            ui.label(RichText::new(format!("+{}", (c.mastery - b) as i64)).color(p.green).size(13.0));
                        }
                        ui.label(RichText::new(format!("{} {}%", "👑".repeat(c.level() as usize), c.mastery as i64)).size(13.0));
                    });
                });
                widgets::progress(ui, c.mastery as f32 / 100.0, p.primary, 10.0, 420.0);
            });
        }
    });
}
