//! Test prep screens: list, new-test setup, and the guided plan page.

use super::widgets::{self, bold, card, card_glow, hex, Mood};
use super::{column_grid, modal_frame, App, Confirm, Dialog, Msg, Pick, Route};
use crate::model::{Concept, Course, Material, Test};
use crate::testprep::{self, Step};
use crate::{actions, ai, game, materials};
use egui::{vec2, Align, Color32, Layout, RichText, Sense, Stroke, Ui};

pub fn countdown(d: Option<i64>) -> (String, String) {
    match d {
        None => ("–".into(), "no date set".into()),
        Some(x) if x < 0 => ("✓".into(), "test date passed".into()),
        Some(0) => ("Today".into(), "good luck! 🍀".into()),
        Some(1) => ("1".into(), "day left".into()),
        Some(x) => (x.to_string(), "days left".into()),
    }
}

pub struct NewTest {
    pub name: String,
    pub date: jiff::civil::Date,
    pub has_date: bool,
    pub course: Option<i64>, // None = new course
    pub new_course: String,
    pub topics: String,
}

pub fn open_new(app: &mut App, course: Option<i64>) {
    let courses = Course::all(&app.db);
    let today = jiff::Zoned::now().date();
    app.new_test_files.clear();
    app.dialog = Some(Dialog::NewTest(Box::new(NewTest {
        name: String::new(), date: today.checked_add(jiff::Span::new().days(7)).unwrap_or(today), has_date: true,
        course: course.or(courses.last().map(|c| c.id)), new_course: String::new(), topics: String::new(),
    })));
}

pub fn list(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("Test Prep 🎯").font(bold(32.0)));
            ui.label(RichText::new("Add everything for an upcoming test. Lumi finds every concept on it and preps you one by one until you're ready.").color(p.muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::button(ui, "+ New test", p.primary, true).clicked() {
                open_new(app, None);
            }
        });
    });
    ui.add_space(16.0);
    let tests = Test::all(&app.db);
    column_grid(ui, 3, 16.0, tests.len() + 1, |ui, i, w| {
        if i == tests.len() {
            let r = egui::Frame::new().stroke(Stroke::new(2.0, p.line)).corner_radius(18).inner_margin(18).show(ui, |ui| {
                ui.set_width(w - 40.0);
                ui.set_min_height(120.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new("+").font(bold(40.0)).color(p.muted));
                    ui.label(RichText::new("Prep for a test").font(bold(16.0)).color(p.muted));
                    ui.label(RichText::new("Study guide, past exams, notes").size(13.0).color(p.faint));
                });
            });
            if r.response.interact(Sense::click()).clicked() {
                open_new(app, None);
            }
            return;
        }
        let t = &tests[i];
        let rows = testprep::concepts(&app.db, t.id);
        let ready = testprep::readiness(&rows);
        let cd = countdown(testprep::days_left(t));
        let course = Course::get(&app.db, t.course_id).unwrap_or_default();
        let r = card(&p).show(ui, |ui| {
            ui.set_width(w - 40.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("📝").size(30.0));
                    ui.label(RichText::new(&t.name).font(bold(17.0)));
                    ui.label(RichText::new(format!("{} {}", course.emoji, course.name)).size(13.0).color(p.muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    ui.allocate_ui_with_layout(egui::vec2(90.0, 0.0), Layout::top_down(Align::Max), |ui| {
                        ui.label(RichText::new(&cd.0).font(bold(26.0)));
                        ui.label(RichText::new(&cd.1).size(12.0).color(p.muted));
                    });
                });
            });
            widgets::progress(ui, ready as f32 / 100.0, if ready >= 80 { p.green } else { p.primary }, 10.0, w - 40.0);
            ui.label(RichText::new(format!("{ready}% ready · {}/{} concepts", rows.iter().filter(|r| r.ready).count(), rows.len())).size(13.0).color(p.muted));
        });
        if r.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.go(Route::Test(t.id));
        }
    });
}

pub fn new_test_dialog(app: &mut App, ctx: &egui::Context, nt: &mut NewTest) -> bool {
    let p = app.pal;
    let courses = Course::all(&app.db);
    let mut keep = true;
    modal_frame(ctx, &p, "newtest", 540.0, |ui| {
        ui.label(RichText::new("Prep for a test 📝").font(bold(28.0)));
        ui.label(RichText::new("Add everything you have. Lumi works out what's on the test and builds your prep plan.").color(p.muted));
        ui.add_space(8.0);
        ui.label(RichText::new("Test name").font(bold(14.0)).color(p.muted));
        ui.add(egui::TextEdit::singleline(&mut nt.name).hint_text("e.g. Biology Midterm, Chapter 5 Quiz, SAT Math").desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Test date").font(bold(14.0)).color(p.muted));
            ui.checkbox(&mut nt.has_date, "");
            if nt.has_date {
                ui.add(egui_extras::DatePickerButton::new(&mut nt.date).id_salt("testdate").calendar_week(false));
            } else {
                ui.label(RichText::new("no date").color(p.faint));
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new("Course").font(bold(14.0)).color(p.muted));
        let sel_name = nt.course.and_then(|id| courses.iter().find(|c| c.id == id)).map(|c| format!("{} {}", c.emoji, c.name)).unwrap_or_else(|| "+ New course…".into());
        egui::ComboBox::from_id_salt("tcourse").selected_text(sel_name).width(480.0).show_ui(ui, |ui| {
            for c in &courses {
                ui.selectable_value(&mut nt.course, Some(c.id), format!("{} {}", c.emoji, c.name));
            }
            ui.selectable_value(&mut nt.course, None, "+ New course…");
        });
        if nt.course.is_none() {
            ui.add(egui::TextEdit::singleline(&mut nt.new_course).hint_text("New course name, e.g. Biology 101").desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
        }
        ui.add_space(6.0);
        ui.label(RichText::new("What's on the test? (optional, but it helps a lot)").font(bold(14.0)).color(p.muted));
        ui.add(egui::TextEdit::multiline(&mut nt.topics).hint_text("Paste the study guide, topic list, or what your teacher said: \"Chapters 4-6, focus on cell respiration\"").desired_rows(4).desired_width(f32::INFINITY));
        ui.add_space(8.0);
        egui::Frame::new().stroke(Stroke::new(2.0, p.line)).corner_radius(16).inner_margin(14).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("📎 Add study guide, past tests, notes, slides…").font(bold(15.0)));
                ui.label(RichText::new("PDF, Word, text or photos (or drag files here). The course's existing materials are included too.").size(13.0).color(p.muted));
                if widgets::button_sized(ui, "Choose files", p.surface2, true, 12.0, 0.0).clicked() {
                    app.pick_files(Pick::TestFiles, false);
                }
                for f in &app.new_test_files {
                    ui.label(RichText::new(format!("📄 {}", f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())).size(13.0).color(p.muted));
                }
            });
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, "Cancel", p.surface2, true).clicked() {
                keep = false;
            }
            let ok = !nt.name.trim().is_empty() && (nt.course.is_some() || !nt.new_course.trim().is_empty());
            if widgets::button(ui, "✨ Build my prep plan", p.primary, ok && app.dash.has_key).clicked() {
                keep = false;
                let cid = nt.course.unwrap_or_else(|| actions::new_course(&app.db, &nt.new_course, &format!("Ace {}", nt.name), "📘"));
                let date = if nt.has_date { Some(nt.date.to_string()) } else { None };
                let tid = actions::new_test(&app.db, cid, &nt.name, date.as_deref(), &nt.topics);
                let files = std::mem::take(&mut app.new_test_files);
                analyze(app, tid, files);
            }
        });
    });
    keep
}

/// Add any new files to the course, then let the AI work out what's on the test.
pub fn analyze(app: &mut App, tid: i64, files: Vec<std::path::PathBuf>) {
    app.busy("Lumi is analyzing your test");
    app.spawn(move |db, tx| {
        let Some(t) = Test::get(db, tid) else { return };
        for f in &files {
            match materials::add_file(db, t.course_id, f) {
                Ok(id) => materials::ensure_ready(db, id),
                Err(e) => {
                    let _ = tx.send(Msg::Toast("⚠️".into(), "Skipped a file".into(), e));
                }
            }
        }
        let mids = Material::enabled_ids(db, t.course_id);
        let ids = serde_json::to_string(&mids).unwrap();
        db.exec("UPDATE tests SET material_ids=? WHERE id=?", &[&ids, &tid]);
        let t = Test::get(db, tid).unwrap_or(t);
        let r = Course::get(db, t.course_id).ok_or("course not found".to_string()).and_then(|c| {
            let data = ai::analyze_test(db, &t, &c, &mids).map_err(|e| e.friendly(db.mode() == "free"))?;
            if data["concepts"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
                return Err("Lumi couldn't find concepts for this test. Add a study guide or describe what's on it.".into());
            }
            testprep::save_plan(db, &t, c.id, &data);
            Ok(())
        });
        let _ = tx.send(Msg::Analyzed(tid, r));
    });
}

pub fn on_analyzed(app: &mut App, tid: i64, r: Result<(), String>) {
    app.busy = None;
    match r {
        Ok(()) => {
            let ev = game::award(&app.db, 10, 0, 0);
            app.sound.play("complete");
            app.confetti(100);
            app.handle(&ev, false);
            app.go(Route::Test(tid));
        }
        Err(e) => {
            app.error(&e);
            app.go(Route::Test(tid));
        }
    }
}

pub fn edit_dialog(app: &mut App, ctx: &egui::Context, id: i64, name: &mut String, date: &mut String, topics: &mut String) -> bool {
    let p = app.pal;
    let mut keep = true;
    modal_frame(ctx, &p, "edittest", 520.0, |ui| {
        ui.label(RichText::new("Edit test").font(bold(26.0)));
        ui.add(egui::TextEdit::singleline(name).hint_text("Name").desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
        ui.horizontal(|ui| {
            ui.label("Date (YYYY-MM-DD)");
            ui.add(egui::TextEdit::singleline(date).desired_width(140.0));
        });
        ui.add(egui::TextEdit::multiline(topics).hint_text("What's on the test?").desired_rows(5).desired_width(f32::INFINITY));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, "Cancel", p.surface2, true).clicked() {
                keep = false;
            }
            let valid = date.is_empty() || game::date_of(date).is_some();
            if widgets::button(ui, "Save", p.primary, valid && !name.trim().is_empty()).clicked() {
                let d: Option<&str> = if date.is_empty() { None } else { Some(date) };
                app.db.exec("UPDATE tests SET name=?, test_date=?, topics=? WHERE id=?", &[&name.trim(), &d, &topics.trim(), &id]);
                keep = false;
            }
        });
    });
    keep
}

fn run_step(app: &mut App, t: &Test, step: Step) {
    let back = Some(Route::Test(t.id));
    match step {
        Step::Analyze => analyze(app, t.id, vec![]),
        Step::Lesson(cid) => {
            let sid = actions::new_session(&app.db, t.course_id, Some(cid), Some(t.id));
            app.go(Route::Learn(sid));
        }
        Step::Mock | Step::Done => {
            let n = testprep::concepts(&app.db, t.id).len();
            super::quiz::start(app, super::quiz::Request {
                course_id: Some(t.course_id), test_id: Some(t.id), kind: Some("mock".into()), count: (n * 2).clamp(10, 20), return_to: back, ..Default::default()
            });
        }
        Step::Diagnostic(cid) => super::quiz::start(app, super::quiz::Request {
            course_id: Some(t.course_id), concept_ids: vec![cid], test_id: Some(t.id), kind: Some("diagnostic".into()), count: 4, return_to: back, ..Default::default()
        }),
        Step::Practice(cid) => super::quiz::start(app, super::quiz::Request {
            course_id: Some(t.course_id), concept_ids: vec![cid], test_id: Some(t.id), kind: Some("practice".into()), count: 8, return_to: back, ..Default::default()
        }),
    }
}

pub fn test(app: &mut App, ui: &mut Ui, id: i64) {
    let p = app.pal;
    let Some(t) = Test::get(&app.db, id) else {
        app.go(Route::Tests);
        return;
    };
    let course = Course::get(&app.db, t.course_id).unwrap_or_default();
    let rows = testprep::concepts(&app.db, id);
    let ready = testprep::readiness(&rows);
    let step = testprep::next_step(&t, &rows);
    let (target, done) = testprep::daily_plan(&t, &rows);
    let cd = countdown(testprep::days_left(&t));
    let color = hex(&course.color);
    egui::Frame::new().fill(widgets::lerp(p.surface, color, 0.75)).corner_radius(24).inner_margin(22).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("📝").size(50.0));
            ui.vertical(|ui| {
                ui.label(RichText::new(&t.name).font(bold(30.0)).color(Color32::WHITE));
                let when = t.test_date.as_deref().and_then(game::date_of).map(|d| format!(" · {}", d.format("%A, %B %-d"))).unwrap_or_default();
                ui.label(RichText::new(format!("{} {}{when}", course.emoji, course.name)).color(Color32::from_white_alpha(230)));
                ui.label(RichText::new(format!("{}/{} concepts ready", rows.iter().filter(|r| r.ready).count(), rows.len())).font(bold(13.0)).color(Color32::WHITE));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                widgets::ring(ui, &p, 120.0, ready as f32 / 100.0, &format!("{ready}%"), "ready", if ready >= 80 { p.green } else if ready >= 50 { p.gold } else { p.pink });
                ui.add_space(14.0);
                ui.allocate_ui_with_layout(egui::vec2(110.0, 0.0), Layout::top_down(Align::Center), |ui| {
                    ui.label(RichText::new(&cd.0).font(bold(42.0)).color(Color32::WHITE));
                    ui.label(RichText::new(&cd.1).font(bold(13.0)).color(Color32::WHITE));
                });
            });
        });
    });
    ui.add_space(14.0);
    let concept_title = |cid: i64| rows.iter().find(|r| r.concept_id == cid).map(|r| r.title.clone()).unwrap_or_default();
    let (icon, title, desc, cta) = match &step {
        Step::Diagnostic(c) => ("⚡", format!("Quick check: {}", concept_title(*c)), "A few questions to see what you already know. Ace them and you skip straight ahead.".to_string(), "Start quick check"),
        Step::Lesson(c) => ("📖", format!("Learn: {}", concept_title(*c)), "A focused lesson with Lumi on exactly what the test will ask, plus the common traps.".into(), "Start lesson"),
        Step::Practice(c) => ("🎯", format!("Practice: {}", concept_title(*c)), "Test-style questions. Score 80% or more to mark it ready.".into(), "Start practice"),
        Step::Mock => ("📝", "Full mock exam".into(), format!("Every concept, test-style, scored like the real thing.{} 80%+ means you're ready.", t.mock_last.map(|s| format!(" Last score: {s}%.")).unwrap_or_default()), "Take mock exam"),
        Step::Done => ("🎓", "You're ready for this test!".into(), format!("Mock exam best: {}%. Keep it fresh with another mock exam or a spaced review before test day.", t.mock_best.unwrap_or(0)), "Another mock exam"),
        Step::Analyze => ("🔍", "Analyze your test".into(), "Lumi needs to read your materials to build the prep plan.".into(), "Analyze now"),
    };
    card_glow(&p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            app.mascot(ui, 96.0, if step == Step::Done { Mood::Celebrate } else { Mood::Happy });
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() - 230.0);
                ui.label(RichText::new("NEXT STEP").font(bold(12.0)).color(p.muted));
                ui.label(RichText::new(format!("{icon} {title}")).font(bold(21.0)));
                ui.label(RichText::new(desc).color(p.muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::button_sized(ui, &format!("{cta} →"), p.green, app.dash.has_key, 16.0, 0.0).clicked() {
                    run_step(app, &t, step.clone());
                }
            });
        });
    });
    if !rows.is_empty() {
        ui.add_space(14.0);
        card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("📅 Today's plan: get {target} concept{} test-ready", if target == 1 { "" } else { "s" })).font(bold(15.0)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(format!("{done}/{target} done today")).size(13.0).color(p.muted)));
            });
            widgets::progress(ui, if target > 0 { done as f32 / target as f32 } else { 1.0 }, p.green, 10.0, ui.available_width());
        });
    }
    if !t.overview.is_empty() {
        ui.add_space(14.0);
        column_grid(ui, 2, 16.0, 2, |ui, i, w| {
            card(&p).show(ui, |ui| {
                ui.set_width(w - 40.0);
                ui.set_min_height(120.0);
                if i == 0 {
                    ui.label(RichText::new("🔍 What's on the test").font(bold(16.0)));
                    ui.label(RichText::new(&t.overview).color(p.muted));
                } else {
                    ui.label(RichText::new("💡 Prep tips").font(bold(16.0)));
                    for tip in &t.tips {
                        ui.label(RichText::new(format!("• {tip}")).color(p.muted));
                    }
                }
            });
        });
    }
    super::section(ui, &p, "Concepts on the test");
    let next_cid = match step {
        Step::Diagnostic(c) | Step::Lesson(c) | Step::Practice(c) => Some(c),
        _ => None,
    };
    let mut action: Option<Step> = None;
    for (i, r) in rows.iter().enumerate() {
        let is_next = next_cid == Some(r.concept_id);
        egui::Frame::new().fill(p.surface).stroke(Stroke::new(2.0, if is_next { p.primary } else { p.line })).corner_radius(18).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (c, _) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::hover());
                ui.painter().circle_filled(c.center(), 19.0, if r.ready { p.green } else if is_next { p.primary } else { p.surface3 });
                ui.painter().text(c.center(), egui::Align2::CENTER_CENTER, if r.ready { "✓".into() } else { (i + 1).to_string() }, bold(15.0), Color32::WHITE);
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&r.title).font(bold(15.0)));
                        let (pl, pc) = match r.priority.as_str() { "high" => ("High", p.pink), "low" => ("Low", p.faint), _ => ("Medium", p.gold) };
                        widgets::tag(ui, &format!("{pl} priority"), pc, p.surface2);
                        let (sl, sfg, sbg) = if r.tested_out { ("Tested out ⚡", p.primary2, widgets::lerp(p.surface, p.primary, 0.2)) } else if r.ready { ("Ready", p.green, widgets::lerp(p.surface, p.green, 0.18)) } else if r.status == "learning" { ("Learning", p.gold, widgets::lerp(p.surface, p.gold, 0.16)) } else { ("Not started", p.muted, p.surface3) };
                        widgets::tag(ui, sl, sfg, sbg);
                    });
                    ui.label(RichText::new(if r.why.is_empty() { &r.summary } else { &r.why }).size(13.0).color(p.muted));
                    widgets::progress(ui, r.mastery as f32 / 100.0, p.primary, 6.0, 320.0);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button_sized(ui, "🎯 Practice", p.surface2, app.dash.has_key, 12.0, 0.0).clicked() {
                        action = Some(Step::Practice(r.concept_id));
                    }
                    if widgets::button_sized(ui, "📖 Learn", p.surface2, app.dash.has_key, 12.0, 0.0).clicked() {
                        action = Some(Step::Lesson(r.concept_id));
                    }
                });
            });
        });
        ui.add_space(8.0);
    }
    if let Some(s) = action {
        run_step(app, &t, s);
    }
    if !rows.is_empty() {
        ui.add_space(8.0);
        card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("📝 Mock exam").font(bold(16.0)));
                    ui.label(RichText::new(match (t.mock_best, t.mock_last) {
                        (Some(b), Some(l)) => format!("A full practice test across every concept, in the style of the real one. Best: {b}% · Last: {l}%"),
                        _ => "A full practice test across every concept, in the style of the real one. Best once everything is ready, but you can try anytime.".into(),
                    }).color(p.muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button(ui, "Take mock exam", p.gold, app.dash.has_key).clicked() {
                        run_step(app, &t, Step::Mock);
                    }
                });
            });
        });
    }
    ui.add_space(30.0);
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.link(RichText::new("Delete").color(p.faint)).clicked() {
                app.dialog = Some(Dialog::Confirm { title: format!("Delete “{}”?", t.name), body: "The prep plan is removed. Concepts and mastery stay in the course.".into(), action: Confirm::DeleteTest(t.id) });
            }
            if ui.link(RichText::new("📎 Course materials").color(p.primary2)).clicked() {
                app.go(Route::Course(t.course_id, super::Tab::Materials));
            }
            if ui.link(RichText::new("↻ Re-analyze (after adding materials)").color(p.primary2)).clicked() {
                analyze(app, t.id, vec![]);
            }
            if ui.link(RichText::new("✏️ Edit test details").color(p.primary2)).clicked() {
                app.dialog = Some(Dialog::EditTest { id: t.id, name: t.name.clone(), date: t.test_date.clone().unwrap_or_default(), topics: t.topics.clone() });
            }
        });
    });
    let _ = Concept::get;
}
