//! Smaller tools: focus timer, saved notes, search (Ctrl+K) and backups.

use super::md::{self, Segment};
use super::widgets::{self, bold, card};
use super::{modal_frame, App, Dialog, Msg, Route};
use crate::model::{Course, Material, Session};
use crate::{actions, game, materials};
use egui::{vec2, Align, Key, Layout, RichText, Ui};
use std::time::Duration;

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Desktop notification without blocking the UI.
fn notify_bg(title: &'static str, body: &'static str) {
    std::thread::spawn(move || crate::reminders::notify(title, body));
}

// ---------- focus timer ----------

pub struct Focus {
    /// Unix time it ends (keeps counting while the window is minimized).
    pub ends: i64,
    pub minutes: i64,
    pub is_break: bool,
}

/// Called every frame: wakes up once a second while a timer runs, nothing otherwise.
pub fn focus_tick(app: &mut App) {
    let Some(f) = &app.focus else { return };
    if f.ends > now_ts() {
        app.ctx.request_repaint_after(Duration::from_secs(1));
        return;
    }
    let f = app.focus.take().unwrap();
    if f.is_break {
        app.sound.play("tap");
        app.toast("🍅", "Break's over", "Ready for another focus round?", 1);
        notify_bg("Break's over", "Ready for another focus round with Lumi?");
    } else {
        let ev = game::focus_done(&app.db, f.minutes);
        app.sound.play("complete");
        app.confetti(120);
        app.toast("🍅", &format!("{} minutes of focus! +{} XP", f.minutes, ev.xp), "Take a short break: stretch, drink some water, look away from the screen.", 1);
        notify_bg("Focus session done! 🍅", "Nice work. Take a 5-minute break.");
        app.focus = Some(Focus { ends: now_ts() + game::BREAK_MINUTES * 60, minutes: game::BREAK_MINUTES, is_break: true });
        app.handle(&ev, false);
        app.refresh();
    }
}

/// Compact Pomodoro card at the bottom of the sidebar.
pub fn focus_card(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    card(&p).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // the sidebar stacks bottom-up; this card reads top-down
        ui.vertical(|ui| match &app.focus {
            None => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🍅").size(24.0));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Focus timer").font(bold(13.0)));
                        ui.label(RichText::new(format!("{} min + {} min break", game::FOCUS_MINUTES, game::BREAK_MINUTES)).size(11.0).color(p.faint));
                    });
                });
                if widgets::button_sized(ui, "Start focus", p.flame, true, 12.0, ui.available_width())
                    .on_hover_text("Study with no distractions until the timer rings. Finishing earns XP.")
                    .clicked()
                {
                    app.focus = Some(Focus { ends: now_ts() + game::FOCUS_MINUTES * 60, minutes: game::FOCUS_MINUTES, is_break: false });
                    app.sound.play("tap");
                }
            }
            Some(f) => {
                let left = (f.ends - now_ts()).max(0);
                let color = if f.is_break { p.ice } else { p.flame };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}:{:02}", left / 60, left % 60)).font(bold(26.0)).color(color));
                    ui.label(RichText::new(if f.is_break { "☕ break" } else { "🍅 focus" }).size(12.0).color(p.muted));
                });
                widgets::progress(ui, 1.0 - left as f32 / (f.minutes * 60).max(1) as f32, color, 8.0, ui.available_width());
                ui.add_space(4.0);
                let label = if f.is_break { "Skip break" } else { "Stop" };
                if widgets::button_sized(ui, label, p.surface2, true, 11.0, ui.available_width()).clicked() {
                    app.focus = None;
                }
            }
        });
    });
}

// ---------- saved notes ----------

/// Save one of Lumi's lesson replies to the notes page.
pub fn save_note(app: &mut App, text: &str) {
    let Some(sid) = app.learn.as_ref().map(|s| s.sid) else { return };
    let Some(session) = Session::get(&app.db, sid) else { return };
    let mut parts = vec![];
    for seg in md::segments(text, false) {
        match seg {
            Segment::Md(t) if !t.trim().is_empty() => parts.push(t.trim().to_string()),
            Segment::Check(q) => {
                // keep the check question with its answer, as a quote
                let opts: Vec<&str> = q["options"].as_array().into_iter().flatten().filter_map(|o| o.as_str()).collect();
                let ans = opts.get(q["answer"].as_u64().unwrap_or(0) as usize).copied().unwrap_or("");
                parts.push(format!("> 🧩 {}\n>\n> **Answer:** {ans}", q["question"].as_str().unwrap_or("")));
            }
            _ => {}
        }
    }
    let body = parts.join("\n\n");
    if body.trim().is_empty() {
        return;
    }
    if app.db.int("SELECT COUNT(*) FROM notes WHERE body=?", &[&body]) > 0 {
        app.toast("🔖", "Already in your notes", "", 1);
        return;
    }
    let heading = body.lines().find_map(|l| l.trim_start().strip_prefix('#').map(|h| h.trim_start_matches('#').trim().to_string())).filter(|h| !h.is_empty());
    let title: String = heading.unwrap_or_else(|| session.title.clone()).chars().take(120).collect();
    app.db.exec("INSERT INTO notes(course_id, concept_id, session_id, title, body) VALUES(?,?,?,?,?)", &[&session.course_id, &session.concept_id, &sid, &title, &body]);
    app.toast("🔖", "Saved to Notes", "Find it any time in Notes (left sidebar).", 1);
    app.sound.play("tap");
    let mut ev = game::Events::default();
    if app.db.int("SELECT COUNT(*) FROM notes", &[]) >= 5 {
        game::unlock(&app.db, "note_taker", &mut ev);
    }
    app.handle(&ev, true);
}

struct Note {
    id: i64,
    course_id: i64,
    title: String,
    body: String,
    created: String,
}

fn load_notes(app: &App, course: Option<i64>) -> Vec<Note> {
    let row = |r: &rusqlite::Row| Ok(Note { id: r.get(0)?, course_id: r.get::<_, Option<i64>>(1)?.unwrap_or(0), title: r.get(2)?, body: r.get(3)?, created: r.get(4)? });
    let sql = "SELECT id, course_id, title, body, created FROM notes";
    match course {
        Some(c) => app.db.query(&format!("{sql} WHERE course_id=? ORDER BY id DESC"), &[&c], row),
        None => app.db.query(&format!("{sql} ORDER BY id DESC"), &[], row),
    }
}

pub fn notes(app: &mut App, ui: &mut Ui, filter: Option<i64>) {
    let p = app.pal;
    super::heading(ui, &p, "Notes 🔖", "Explanations you saved from lessons. Press 🔖 Save under any of Lumi's answers to keep it here.");
    let all = load_notes(app, None);
    let courses: Vec<Course> = Course::all(&app.db).into_iter().filter(|c| all.iter().any(|n| n.course_id == c.id)).collect();
    let notes: Vec<&Note> = all.iter().filter(|n| filter.is_none_or(|c| n.course_id == c)).collect();
    if all.is_empty() {
        card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                app.mascot(ui, 120.0, widgets::Mood::Happy);
                ui.label(RichText::new("No notes yet").font(bold(20.0)));
                ui.label(RichText::new("When Lumi explains something well, press 🔖 Save under the message. It's a quick way to build your own study guide.").color(p.muted));
            });
        });
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if ui.selectable_label(filter.is_none(), format!("All ({})", all.len())).clicked() {
            app.go(Route::Notes(None));
        }
        for c in &courses {
            let n = all.iter().filter(|x| x.course_id == c.id).count();
            if ui.selectable_label(filter == Some(c.id), format!("{} {} ({n})", c.emoji, c.name)).clicked() {
                app.go(Route::Notes(Some(c.id)));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::button_sized(ui, "⬇ Export as Markdown", p.surface2, true, 12.0, 0.0).clicked() {
                export_notes(app, filter);
            }
        });
    });
    ui.add_space(10.0);
    let mut delete = None;
    for n in notes {
        let course = courses.iter().find(|c| c.id == n.course_id);
        card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(md::math_to_unicode(&n.title)).font(bold(17.0)));
                    let when = n.created.get(..10).unwrap_or("");
                    ui.label(RichText::new(format!("{} {} · {when}", course.map(|c| c.emoji.as_str()).unwrap_or(""), course.map(|c| c.name.as_str()).unwrap_or(""))).size(12.0).color(p.faint));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("🗑 Delete").clicked() {
                        delete = Some(n.id);
                    }
                    if ui.small_button("📋 Copy").clicked() {
                        ui.ctx().copy_text(n.body.clone());
                        app.toast("📋", "Copied", "", 1);
                    }
                });
            });
            ui.add_space(4.0);
            egui::CollapsingHeader::new(RichText::new("Show note").size(13.0).color(p.muted)).id_salt(("note", n.id)).show(ui, |ui| {
                md::show(ui, &mut app.md_cache, &n.body);
            });
        });
        ui.add_space(10.0);
    }
    if let Some(id) = delete {
        app.db.exec("DELETE FROM notes WHERE id=?", &[&id]);
    }
}

fn export_notes(app: &mut App, course: Option<i64>) {
    let notes = load_notes(app, course);
    let names: std::collections::HashMap<i64, String> = Course::all(&app.db).into_iter().map(|c| (c.id, c.name)).collect();
    let mut out = String::from("# My Lumi notes\n");
    let mut last = -1;
    for n in notes.iter().rev() {
        if n.course_id != last {
            out += &format!("\n## {}\n", names.get(&n.course_id).map(|s| s.as_str()).unwrap_or("Notes"));
            last = n.course_id;
        }
        out += &format!("\n### {}\n\n{}\n", n.title.trim_start_matches('#').trim(), n.body);
    }
    app.spawn(move |_, tx| {
        let Some(path) = rfd::FileDialog::new().set_file_name("lumi-notes.md").add_filter("Markdown", &["md"]).save_file() else { return };
        let msg = match std::fs::write(&path, out) {
            Ok(()) => Msg::Toast("⬇".into(), "Notes exported".into(), path.display().to_string()),
            Err(e) => Msg::Toast("😵".into(), "Couldn't save the file".into(), e.to_string()),
        };
        let _ = tx.send(msg);
    });
}

// ---------- search ----------

#[derive(Default)]
pub struct Search {
    pub query: String,
    hits: Vec<Hit>,
    searched: String,
    sel: usize,
    focused: bool,
}

struct Hit {
    icon: String,
    title: String,
    sub: String,
    go: Go,
}

enum Go {
    Course(i64),
    Concept(i64, i64),
    Lesson(i64),
    Test(i64),
    Note(i64),
    Page { course: i64, file: String, label: String },
}

pub fn open_search(app: &mut App) {
    if !matches!(app.dialog, Some(Dialog::Search(_))) {
        app.dialog = Some(Dialog::Search(Box::default()));
    }
}

/// "photo synth" -> `"photo"* "synth"*` (safe for FTS5: no operators from the user).
fn fts_query(q: &str) -> String {
    materials::tokens(q).iter().map(|t| format!("\"{t}\"*")).collect::<Vec<_>>().join(" ")
}

fn run_search(app: &App, q: &str) -> Vec<Hit> {
    let db = &app.db;
    let like = format!("%{}%", q.trim());
    let mut hits = vec![];
    for (id, name, emoji) in db.query("SELECT id, name, emoji FROM courses WHERE name LIKE ? LIMIT 4", &[&like], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))) {
        hits.push(Hit { icon: emoji.unwrap_or("📘".into()), title: name, sub: "Course".into(), go: Go::Course(id) });
    }
    for (id, name) in db.query("SELECT id, name FROM tests WHERE name LIKE ? LIMIT 3", &[&like], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))) {
        hits.push(Hit { icon: "🎯".into(), title: name, sub: "Test".into(), go: Go::Test(id) });
    }
    for (id, cid, title, unit) in db.query("SELECT id, course_id, title, unit_title FROM concepts WHERE title LIKE ? OR summary LIKE ? ORDER BY title LIKE ? DESC LIMIT 6", &[&like, &like, &like],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?)))
    {
        hits.push(Hit { icon: "💡".into(), title, sub: format!("Concept · {}", unit.unwrap_or_default()), go: Go::Concept(cid, id) });
    }
    for (id, cid, title) in db.query("SELECT id, course_id, title FROM notes WHERE title LIKE ? OR body LIKE ? ORDER BY id DESC LIMIT 4", &[&like, &like],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0), r.get::<_, String>(2)?)))
    {
        let _ = id;
        hits.push(Hit { icon: "🔖".into(), title, sub: "Saved note".into(), go: Go::Note(cid) });
    }
    for (id, title) in db.query("SELECT id, title FROM sessions WHERE title LIKE ? ORDER BY updated_at DESC LIMIT 4", &[&like], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))) {
        hits.push(Hit { icon: "💬".into(), title, sub: "Lesson".into(), go: Go::Lesson(id) });
    }
    let fq = fts_query(q);
    if !fq.is_empty() {
        let rows = db.query(
            "SELECT material_id, page, snippet(pages_fts, 0, '', '', '…', 14) FROM pages_fts WHERE pages_fts MATCH ? ORDER BY bm25(pages_fts) LIMIT 8",
            &[&fq],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)),
        );
        for (mid, page, snip) in rows {
            let Some(m) = Material::get(db, mid).filter(|m| m.enabled) else { continue };
            let labels: Vec<String> = m.page_labels.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
            let label = labels.get(page as usize - 1).cloned().unwrap_or(page.to_string());
            let snip: String = snip.split_whitespace().collect::<Vec<_>>().join(" ");
            hits.push(Hit { icon: "📖".into(), title: format!("{} · p. {label}", m.filename), sub: snip, go: Go::Page { course: m.course_id, file: m.filename.clone(), label } });
        }
    }
    hits
}

fn open_hit(app: &mut App, go: &Go) {
    match go {
        Go::Course(id) => app.go(Route::Course(*id, super::Tab::Path)),
        Go::Concept(course, concept) => {
            app.go(Route::Course(*course, super::Tab::Path));
            app.dialog = Some(Dialog::ConceptMenu { course: *course, concept: *concept });
        }
        Go::Lesson(id) => app.go(Route::Learn(*id)),
        Go::Test(id) => app.go(Route::Test(*id)),
        Go::Note(course) => app.go(Route::Notes(Some(*course).filter(|c| *c > 0))),
        Go::Page { course, file, label } => {
            if !app.dash.has_key {
                app.error("Add an AI key in Settings to have Lumi explain a page.");
                return;
            }
            let sid = actions::new_session(&app.db, *course, None, None);
            app.go(Route::Learn(sid));
            super::learn::open_with(app, sid, format!("Walk me through page {label} of \"{file}\". What's important on it?"));
        }
    }
}

/// The Ctrl+K palette. Returns false to close.
pub fn search_dialog(app: &mut App, ctx: &egui::Context, st: &mut Search) -> bool {
    let p = app.pal;
    let mut keep = true;
    let mut open: Option<usize> = None;
    if st.query.trim() != st.searched {
        st.searched = st.query.trim().to_string();
        st.hits = if st.searched.chars().count() >= 2 { run_search(app, &st.searched) } else { vec![] };
        st.sel = 0;
    }
    let (down, up, enter, esc) = ctx.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    if esc {
        return false;
    }
    if !st.hits.is_empty() {
        if down {
            st.sel = (st.sel + 1).min(st.hits.len() - 1);
        }
        if up {
            st.sel = st.sel.saturating_sub(1);
        }
        if enter {
            open = Some(st.sel);
        }
    }
    let r = modal_frame(ctx, &p, "search", 640.0, |ui| {
        let edit = ui.add(egui::TextEdit::singleline(&mut st.query).hint_text("Search concepts, notes, lessons and your textbooks…").font(egui::FontId::proportional(18.0)).desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
        if !st.focused {
            edit.request_focus();
            st.focused = true;
        }
        ui.add_space(8.0);
        if st.searched.chars().count() < 2 {
            ui.label(RichText::new("Type at least 2 letters. ↑↓ to move, Enter to open, Esc to close.").size(12.0).color(p.faint));
        } else if st.hits.is_empty() {
            ui.label(RichText::new("Nothing found. Try another word.").color(p.muted));
        }
        for (i, h) in st.hits.iter().enumerate() {
            let selected = i == st.sel;
            let resp = egui::Frame::new().fill(if selected { p.surface2 } else { p.surface }).corner_radius(10).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&h.icon).size(20.0));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(md::math_to_unicode(&h.title)).font(bold(14.0)));
                        ui.add(egui::Label::new(RichText::new(&h.sub).size(12.0).color(p.faint)).truncate());
                    });
                });
            });
            let resp = resp.response.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if selected && (down || up) {
                resp.scroll_to_me(None);
            }
            if resp.clicked() {
                open = Some(i);
            }
        }
    });
    if r.clicked_elsewhere() && open.is_none() {
        keep = false;
    }
    if let Some(i) = open {
            let hit = st.hits.swap_remove(i);
        open_hit(app, &hit.go); // may open another dialog, which is kept
        return false;
    }
    keep
}

// ---------- backups (Settings) ----------

pub fn backup_section(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    ui.label(RichText::new("Save everything (courses, files, progress, notes, shop items) to one file, to keep safe or move to another computer. AI keys are left out on purpose.").color(p.muted));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if widgets::button_sized(ui, "📦 Export backup", p.primary, true, 13.0, 0.0).clicked() {
            app.spawn(|db, tx| {
                let Some(path) = rfd::FileDialog::new().set_file_name(crate::backup::default_name()).add_filter("Lumi backup", &["zip"]).save_file() else { return };
                let msg = match crate::backup::export(db, &path) {
                    Ok(()) => Msg::Toast("📦".into(), "Backup saved".into(), path.display().to_string()),
                    Err(e) => Msg::Toast("😵".into(), "Backup failed".into(), e),
                };
                let _ = tx.send(msg);
            });
        }
        if widgets::button_sized(ui, "Restore from backup…", p.surface2, true, 13.0, 0.0).clicked() {
            app.spawn(|db, tx| {
                let Some(path) = rfd::FileDialog::new().add_filter("Lumi backup", &["zip"]).pick_file() else { return };
                let msg = match crate::backup::stage_restore(&db.path(""), &path) {
                    Ok(()) => Msg::Toast("📦".into(), "Backup ready: restart Lumi to finish".into(), "Your current data will be kept in a \"before-restore\" folder, just in case.".into()),
                    Err(e) => Msg::Toast("😵".into(), "Couldn't restore".into(), e),
                };
                let _ = tx.send(msg);
            });
        }
    });
}
