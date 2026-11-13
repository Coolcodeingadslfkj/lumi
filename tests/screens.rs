//! Renders every screen headlessly with sample data, to catch crashes and layout problems.
//! Images go to target/screens/. Needs a GPU or software Vulkan (wgpu); skips if none.
//! Run: cargo test --release --test screens -- --nocapture

use lumi::db::Db;
use lumi::ui::{App, Route, Tab};
use serde_json::json;
use std::path::PathBuf;

fn seed(db: &Db) {
    db.set("welcomed", &true);
    db.set("provider", &"free");
    db.set("groq_key", &Some("test-key-not-real"));
    db.set("name", &"Sam");
    db.set("gems", &145);
    db.set("owned", &vec!["wizard"]);
    db.set("hat", &Some("wizard"));
    let cid = lumi::actions::new_course(db, "Discrete Math", "Ace the midterm", "🧮");
    let units = [("Logic & Proofs", ["Propositional Logic", "Direct Proof", "Contraposition"]), ("Induction", ["Weak Induction", "Strong Induction", "Recursion"])];
    for (ui, (unit, concepts)) in units.iter().enumerate() {
        for (ci, t) in concepts.iter().enumerate() {
            let m = [85.0, 45.0, 10.0, 0.0, 0.0, 0.0][ui * 3 + ci];
            db.exec("INSERT INTO concepts(course_id, unit_idx, unit_title, idx, title, summary, mastery, attempts) VALUES(?,?,?,?,?,?,?,?)",
                &[&cid, &(ui as i64), unit, &(ci as i64), t, &format!("Use {t} in proofs (Section {}.{}).", ui + 4, ci + 1), &m, &((m > 0.0) as i64)]);
        }
    }
    lumi::materials::add_note(db, cid, "Lecture notes", "Contrapositive: if not q then not p. Induction: base case and inductive step.");
    let tid = lumi::actions::new_test(db, cid, "Midterm 2", Some(&(chrono::Local::now().date_naive() + chrono::Duration::days(5)).format("%Y-%m-%d").to_string()), "Ch 4-5");
    let t = lumi::model::Test::get(db, tid).unwrap();
    lumi::testprep::save_plan(db, &t, cid, &json!({"overview": "Proof techniques and induction, mostly written proofs.", "tips": ["Memorize the contrapositive form", "Practice induction base cases"],
        "concepts": [{"existing_concept_id": 2, "title": "Direct Proof", "summary": "", "priority": "high", "why": "Quiz 3 Q1"},
                     {"existing_concept_id": 3, "title": "Contraposition", "summary": "", "priority": "high", "why": "Quiz 3 Q4"},
                     {"existing_concept_id": 0, "title": "Pigeonhole Principle", "summary": "Apply it.", "priority": "low", "why": "Notes"}]}));
    db.exec("INSERT INTO mistakes(course_id, concept_id, question, user_answer) VALUES(?,?,?,?)",
        &[&cid, &2i64, &json!({"type": "fill_blank", "prompt": "The contrapositive of $p \\to q$ is ___.", "accepted_answers": ["not q implies not p"], "explanation": "Swap and negate."}).to_string(), &"q implies p"]);
    for d in 0..5 {
        let day = (chrono::Local::now().date_naive() - chrono::Duration::days(d)).format("%Y-%m-%d").to_string();
        db.exec("INSERT INTO activity(day, xp, answers, correct) VALUES(?,?,10,8)", &[&day, &(30 + d * 15)]);
    }
    let sid = lumi::actions::new_session(db, cid, Some(2), None);
    let msgs = json!([
        {"role": "user", "content": [{"type": "text", "text": "Teach me contraposition"}]},
        {"role": "assistant", "via": "gemini", "content": [{"type": "text", "text": "**Contraposition** says $p \\to q$ is equivalent to $\\neg q \\to \\neg p$.\n\nFor all integers $n$, if $n^2$ is even then $n$ is even.\n\n```check\n{\"question\": \"What is the contrapositive of p → q?\", \"options\": [\"q → p\", \"¬q → ¬p\", \"¬p → ¬q\"], \"answer\": 1, \"explanation\": \"Swap and negate both.\"}\n```"}]}
    ]);
    db.exec("UPDATE sessions SET messages=? WHERE id=?", &[&msgs.to_string(), &sid]);
    db.exec("INSERT INTO notes(course_id, concept_id, session_id, title, body) VALUES(?,?,?,?,?)",
        &[&cid, &3i64, &sid, &"Contraposition in one line", &"**Contraposition:** $p \\to q$ is equivalent to $\\neg q \\to \\neg p$.\n\n> 🧩 What is the contrapositive of p → q?\n>\n> **Answer:** ¬q → ¬p"]);
    // a few answered questions, so Lightning Rounds unlock
    for (i, (prompt, ans)) in [("$2+2=4$", 0), ("Every even number is divisible by 4.", 1), ("$\\neg(p \\land q) \\equiv \\neg p \\lor \\neg q$", 0), ("Induction needs a base case.", 0), ("$\\sqrt{2}$ is rational.", 1), ("A proof by contraposition proves $\\neg q \\to \\neg p$.", 0)].iter().enumerate() {
        lumi::game::bank_question(db, cid, Some((i % 3 + 1) as i64), &json!({"type": "true_false", "prompt": prompt, "options": ["True", "False"], "correct_index": ans, "explanation": ""}));
    }
}

#[test]
fn render_all_screens() {
    let dir = std::env::temp_dir().join(format!("lumi-screens-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: set before any other threads start in this test
    unsafe { std::env::set_var("LUMI_NO_AUTOSTART", "1") };
    let db = Db::open_at(&dir).unwrap();
    seed(&db);
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screens");
    std::fs::create_dir_all(&out).unwrap();
    let routes = [
        ("home", Route::Home), ("courses", Route::Courses), ("path", Route::Course(1, Tab::Path)), ("materials", Route::Course(1, Tab::Materials)),
        ("lessons", Route::Course(1, Tab::Lessons)), ("learn", Route::Learn(1)), ("tests", Route::Tests), ("test", Route::Test(1)),
        ("review", Route::Review), ("mistakes", Route::Mistakes(None)), ("achievements", Route::Achievements), ("shop", Route::Shop), ("settings", Route::Settings), ("notes", Route::Notes(None)),
    ];
    let mut app: Option<App> = None;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1360.0, 900.0)).wgpu().build_ui(|ui| {
        let a = app.get_or_insert_with(|| App::with_ctx(ui.ctx().clone(), db.clone()));
        a.frame(ui);
    });
    for (name, route) in routes {
        harness.step();
        // switch screens between frames
        harness.run_steps(1);
        set_route(&mut harness, route);
        harness.run_steps(4);
        match harness.render() {
            Ok(img) => img.save(out.join(format!("{name}.png"))).unwrap(),
            Err(e) => {
                eprintln!("skipping screenshots: no renderer ({e})");
                return;
            }
        }
    }
}

fn set_route(h: &mut egui_kittest::Harness, r: Route) {
    // The harness owns the app; hand the route over through the context's temp data.
    h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("test_route"), r));
}

fn q(t: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({"type": t, "concept_id": 2, "prompt": "", "options": [], "correct_index": 0, "accepted_answers": [], "pairs": [], "ordered_items": [],
        "model_answer": "", "rubric": "", "hint": "Think about swapping.", "explanation": "The contrapositive swaps and negates: $\\neg q \\to \\neg p$."});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

#[test]
fn render_quiz_screens() {
    use egui::Key;
    let dir = std::env::temp_dir().join(format!("lumi-quiz-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    unsafe { std::env::set_var("LUMI_NO_AUTOSTART", "1") };
    let db = Db::open_at(&dir).unwrap();
    seed(&db);
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screens");
    std::fs::create_dir_all(&out).unwrap();
    let concepts = lumi::model::Concept::for_course(&db, 1);
    let questions = vec![
        q("multiple_choice", json!({"prompt": "What is the contrapositive of $p \\to q$?", "options": ["$q \\to p$", "$\\neg q \\to \\neg p$", "$\\neg p \\to \\neg q$"], "correct_index": 1})),
        q("true_false", json!({"prompt": "A statement and its contrapositive are logically equivalent.", "options": ["True", "False"], "correct_index": 0})),
        q("fill_blank", json!({"prompt": "If $n^2$ is even then $n$ is ___.", "accepted_answers": ["even"]})),
        q("match_pairs", json!({"prompt": "Match each term", "pairs": [{"left": "Converse", "right": "q → p"}, {"left": "Inverse", "right": "¬p → ¬q"}, {"left": "Contrapositive", "right": "¬q → ¬p"}]})),
        q("order_steps", json!({"prompt": "Order the proof steps", "ordered_items": ["Assume n is odd", "Write n = 2k+1", "Square it", "Conclude n² is odd"]})),
        q("short_answer", json!({"prompt": "Explain why proving the contrapositive proves the original statement.", "model_answer": "They are logically equivalent.", "rubric": "equivalence"})),
    ];
    let mut app: Option<App> = None;
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1360.0, 900.0)).wgpu().build_ui(|ui| {
        let a = app.get_or_insert_with(|| App::with_ctx(ui.ctx().clone(), db.clone()));
        a.frame(ui);
    });
    let shot = |h: &mut egui_kittest::Harness, name: &str| {
        h.run_steps(4);
        if let Ok(img) = h.render() {
            img.save(out.join(format!("{name}.png"))).unwrap();
        }
    };
    let start = |h: &mut egui_kittest::Harness, qs: Vec<serde_json::Value>, mode: &str, boss: bool| {
        let req = lumi::ui::quiz::Request { course_id: Some(1), mode: mode.into(), boss: if boss { Some(("Fog Phantom".into(), egui::Color32::from_rgb(124, 92, 255), 0)) } else { None }, kind: if boss { Some("boss".into()) } else { None }, ..Default::default() };
        let data = lumi::ui::quiz::Data { course_id: 1, concepts: concepts.clone(), questions: qs, via: "Groq".into() };
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("test_quiz"), Some((req, data))));
    };
    h.run_steps(2);
    for (i, name) in ["q_mc", "q_tf", "q_blank", "q_match", "q_order", "q_short"].iter().enumerate() {
        start(&mut h, vec![questions[i].clone()], "practice", false);
        shot(&mut h, name);
    }
    // answer a multiple-choice question wrong, then see feedback and results
    start(&mut h, vec![questions[0].clone()], "challenge", false);
    h.run_steps(3);
    h.key_press(Key::Num1);
    h.run_steps(2);
    h.key_press(Key::Enter);
    shot(&mut h, "q_feedback_wrong");
    start(&mut h, vec![questions[1].clone(), questions[0].clone()], "boss", true);
    h.run_steps(3);
    h.key_press(Key::Num1);
    h.run_steps(2);
    h.key_press(Key::Enter);
    shot(&mut h, "q_boss_hit");
    h.key_press(Key::Enter);
    h.run_steps(3);
    h.key_press(Key::Num2);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    h.key_press(Key::Enter);
    shot(&mut h, "q_boss_results");
    // lightning round from the question bank (works without AI)
    h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("test_quiz"), None::<(lumi::ui::quiz::Request, lumi::ui::quiz::Data)>));
    let (cid, qs) = lumi::game::lightning_questions(&db, Some(1)).expect("bank has questions");
    let req = lumi::ui::quiz::Request { course_id: Some(cid), mode: "lightning".into(), kind: Some("lightning".into()), ..Default::default() };
    h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("test_quiz"), Some((req, lumi::ui::quiz::Data { course_id: cid, concepts: vec![], questions: qs, via: "Your question bank".into() }))));
    h.run_steps(3);
    shot(&mut h, "q_lightning");
}

#[test]
fn render_search_and_focus() {
    let dir = std::env::temp_dir().join(format!("lumi-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    unsafe { std::env::set_var("LUMI_NO_AUTOSTART", "1") };
    let db = Db::open_at(&dir).unwrap();
    seed(&db);
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screens");
    std::fs::create_dir_all(&out).unwrap();
    let mut app: Option<App> = None;
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1360.0, 900.0)).wgpu().build_ui(|ui| {
        let a = app.get_or_insert_with(|| App::with_ctx(ui.ctx().clone(), db.clone()));
        a.frame(ui);
    });
    h.run_steps(3);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    h.run_steps(3);
    h.event(egui::Event::Text("contra".into()));
    h.run_steps(4);
    if let Ok(img) = h.render() {
        img.save(out.join("search.png")).unwrap();
    }
}

/// The whole shop in one tall image (target/screens/shop_full.png), for previewing new items.
#[test]
fn render_shop_catalog() {
    let dir = std::env::temp_dir().join(format!("lumi-shop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    unsafe { std::env::set_var("LUMI_NO_AUTOSTART", "1") };
    let db = Db::open_at(&dir).unwrap();
    seed(&db);
    db.set("accessory", &Some("bowtie"));
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screens");
    std::fs::create_dir_all(&out).unwrap();
    let mut app: Option<App> = None;
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1360.0, 4200.0)).wgpu().build_ui(|ui| {
        let a = app.get_or_insert_with(|| App::with_ctx(ui.ctx().clone(), db.clone()));
        a.frame(ui);
    });
    h.run_steps(2);
    set_route(&mut h, Route::Shop);
    h.run_steps(5);
    if let Ok(img) = h.render() {
        img.save(out.join("shop_full.png")).unwrap();
    }
}

fn click(h: &mut egui_kittest::Harness, x: f32, y: f32) {
    let pos = egui::pos2(x, y);
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.run_steps(1);
    }
}

/// Regression: leaving a finished test-prep quiz or lesson used to close the app,
/// and leaving a lesson didn't count it in the plan.
#[test]
fn leaving_test_prep_keeps_app_and_progress() {
    let dir = std::env::temp_dir().join(format!("lumi-leave-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    unsafe { std::env::set_var("LUMI_NO_AUTOSTART", "1") };
    let db = Db::open_at(&dir).unwrap();
    seed(&db);
    let tid = 1i64;
    let concept = db.int("SELECT concept_id FROM test_concepts WHERE test_id=? ORDER BY idx LIMIT 1", &[&tid]);
    let mut app: Option<App> = None;
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1360.0, 900.0)).wgpu().build_ui(|ui| {
        let a = app.get_or_insert_with(|| App::with_ctx(ui.ctx().clone(), db.clone()));
        a.frame(ui);
    });
    h.run_steps(2);
    set_route(&mut h, Route::Test(tid));
    h.run_steps(2);
    // finish a practice quiz, then click "Back to test plan" in the footer
    let q = json!({"type": "true_false", "concept_id": concept, "prompt": "Q?", "options": ["True", "False"], "correct_index": 0, "explanation": "x"});
    let req = lumi::ui::quiz::Request { course_id: Some(1), test_id: Some(tid), kind: Some("practice".into()), mode: "practice".into(), return_to: Some(Route::Test(tid)), ..Default::default() };
    let data = lumi::ui::quiz::Data { course_id: 1, concepts: lumi::model::Concept::many(&db, &[concept]), questions: vec![q.clone(), q.clone(), q], via: "x".into() };
    h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("test_quiz"), Some((req, data))));
    h.run_steps(3);
    for _ in 0..3 {
        h.key_press(egui::Key::Num1);
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
    }
    h.run_steps(5);
    click(&mut h, 680.0, 558.0); // "Keep going" on the daily-goal celebration
    h.run_steps(5);
    if let Ok(img) = h.render() { img.save("/tmp/before_back.png").unwrap(); }
    click(&mut h, 1090.0, 846.0);
    h.run_steps(3);
    let status: String = db.one("SELECT status FROM test_concepts WHERE test_id=? AND concept_id=?", &[&tid, &concept], |r| r.get(0)).unwrap();
    assert_eq!(status, "ready", "the practice result is saved");

    // a test-prep lesson that Lumi has taught, then the ← button
    let c2 = db.int("SELECT concept_id FROM test_concepts WHERE test_id=? AND status!='ready' ORDER BY idx LIMIT 1", &[&tid]);
    let sid = lumi::actions::new_session(&db, 1, Some(c2), Some(tid));
    let msgs = json!([{"role": "user", "content": [{"type": "text", "text": "Teach me"}]}, {"role": "assistant", "via": "gemini", "content": [{"type": "text", "text": "Here's the idea."}]}]);
    db.exec("UPDATE sessions SET messages=? WHERE id=?", &[&msgs.to_string(), &sid]);
    set_route(&mut h, Route::Learn(sid));
    h.run_steps(4);
    click(&mut h, 273.0, 89.0);
    h.run_steps(3);
    let done = db.int("SELECT lesson_done FROM test_concepts WHERE test_id=? AND concept_id=?", &[&tid, &c2]);
    assert_eq!(done, 1, "leaving a taught lesson counts it");
    if let Ok(img) = h.render() {
        img.save(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screens/after_leaving_lesson.png")).unwrap();
    }
}
