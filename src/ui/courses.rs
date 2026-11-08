//! Courses: list, learning path (with bosses), materials, lessons.

use super::widgets::{self, bold, card, card_glow, hex, BossMood, Mood};
use super::{column_grid, modal_frame, App, Dialog, Msg, Pick, Route, Tab};
use crate::model::{Concept, Course, Material, Session};
use crate::{actions, ai, game, materials};
use egui::{vec2, Align, Align2, Color32, Layout, RichText, Sense, Stroke, Ui};
use serde_json::Value;

pub fn list(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    super::heading(ui, &p, "Courses", "Each course is a subject with its own materials, learning path and mastery.");
    cards(app, ui);
}

pub fn cards(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let courses = Course::all(&app.db);
    let n = courses.len() + 1;
    column_grid(ui, 4, 16.0, n, |ui, i, w| {
        if i == courses.len() {
            let r = egui::Frame::new().stroke(Stroke::new(2.0, p.line)).corner_radius(18).inner_margin(18).show(ui, |ui| {
                ui.set_width(w - 40.0);
                ui.set_min_height(140.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(14.0);
                    ui.label(RichText::new("+").font(bold(40.0)).color(p.muted));
                    ui.label(RichText::new("New course").font(bold(16.0)).color(p.muted));
                    ui.label(RichText::new("Upload a textbook, notes or quizzes").size(13.0).color(p.faint));
                });
            });
            if r.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.dialog = Some(Dialog::NewCourse { name: String::new(), goal: String::new(), emoji: "📘".into() });
            }
            return;
        }
        let c = &courses[i];
        let concepts = Concept::for_course(&app.db, c.id);
        let progress = if concepts.is_empty() { 0.0 } else { concepts.iter().map(|x| x.mastery).sum::<f64>() / concepts.len() as f64 };
        let mats = app.db.int("SELECT COUNT(*) FROM materials WHERE course_id=?", &[&c.id]);
        let due = game::due_concepts(&app.db, Some(c.id)).len();
        let r = card(&p).show(ui, |ui| {
            ui.set_width(w - 40.0);
            ui.set_min_height(140.0);
            let top = ui.min_rect();
            ui.painter().rect_filled(egui::Rect::from_min_size(top.min - vec2(18.0, 18.0), vec2(w - 4.0, 6.0)), egui::CornerRadius { nw: 16, ne: 16, sw: 0, se: 0 }, hex(&c.color));
            ui.label(RichText::new(&c.emoji).size(36.0));
            ui.label(RichText::new(&c.name).font(bold(17.0)));
            ui.horizontal_wrapped(|ui| {
                widgets::tag(ui, &format!("{mats} material{}", if mats == 1 { "" } else { "s" }), p.muted, p.surface2);
                widgets::tag(ui, &format!("{} concepts", concepts.len()), p.muted, p.surface2);
                if due > 0 {
                    widgets::tag(ui, &format!("{due} due"), p.pink, widgets::lerp(p.surface, p.pink, 0.18));
                }
            });
            ui.add_space(6.0);
            widgets::progress(ui, progress as f32 / 100.0, p.primary, 10.0, w - 40.0);
            ui.label(RichText::new(format!("{}% mastered", progress as i64)).size(13.0).color(p.muted));
        });
        if r.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.go(Route::Course(c.id, Tab::Path));
        }
    });
}

pub fn course(app: &mut App, ui: &mut Ui, id: i64, mut tab: Tab) {
    let p = app.pal;
    let Some(c) = Course::get(&app.db, id) else {
        app.go(Route::Courses);
        return;
    };
    let concepts = Concept::for_course(&app.db, id);
    let mats = Material::for_course(&app.db, id);
    if tab == Tab::Path && concepts.is_empty() && mats.is_empty() {
        tab = Tab::Materials;
    }
    let progress = if concepts.is_empty() { 0.0 } else { concepts.iter().map(|x| x.mastery).sum::<f64>() / concepts.len() as f64 };
    let due = game::due_concepts(&app.db, Some(id)).len();
    let color = hex(&c.color);
    egui::Frame::new().fill(widgets::lerp(p.surface, color, 0.75)).corner_radius(24).inner_margin(22).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&c.emoji).size(50.0));
            ui.vertical(|ui| {
                ui.label(RichText::new(&c.name).font(bold(30.0)).color(Color32::WHITE));
                if !c.goal.is_empty() {
                    ui.label(RichText::new(format!("🎯 {}", c.goal)).color(Color32::from_white_alpha(230)));
                }
                widgets::progress(ui, progress as f32 / 100.0, Color32::WHITE, 10.0, 320.0);
                ui.label(RichText::new(format!("{}% mastered · {} concepts", progress as i64, concepts.len())).font(bold(13.0)).color(Color32::WHITE));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if due > 0 && widgets::button(ui, &format!("🧠 Review {due}"), p.surface2, true).clicked() {
                    super::quiz::start(app, super::quiz::Request { course_id: Some(id), review: true, ..Default::default() });
                }
                if !concepts.is_empty() && widgets::button(ui, "⚡ Practice", p.gold, true).clicked() {
                    super::quiz::start(app, super::quiz::Request { course_id: Some(id), ..Default::default() });
                }
            });
        });
    });
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        for (t, label) in [(Tab::Path, "🗺️ Path"), (Tab::Materials, "📎 Materials"), (Tab::Lessons, "💬 Lessons")] {
            let sel = t == tab;
            let r = egui::Frame::new().fill(if sel { p.surface3 } else { p.surface }).stroke(Stroke::new(2.0, p.line)).corner_radius(12).inner_margin(egui::Margin::symmetric(16, 8))
                .show(ui, |ui| ui.label(RichText::new(label).font(bold(15.0)).color(if sel { p.text } else { p.muted })));
            if r.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.go(Route::Course(id, t));
            }
        }
    });
    ui.add_space(14.0);
    match tab {
        Tab::Path => path(app, ui, &c, &concepts, mats.is_empty()),
        Tab::Materials => materials_tab(app, ui, &c, &mats, concepts.is_empty()),
        Tab::Lessons => lessons(app, ui, &c),
    }
    ui.add_space(40.0);
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        if ui.link(RichText::new("Delete course").color(p.faint)).clicked() {
            app.dialog = Some(Dialog::Confirm {
                title: format!("Delete “{}”?", c.name),
                body: "This removes its materials, learning path, lessons and mastery. Your XP and streak stay.".into(),
                action: super::Confirm::DeleteCourse(c.id),
            });
        }
    });
}

pub fn generate_path(app: &mut App, course_id: i64) {
    app.busy("Lumi is designing your learning path");
    app.spawn(move |db, tx| {
        let r = Course::get(db, course_id).ok_or("course not found".to_string()).and_then(|c| ai::generate_curriculum(db, &c).map_err(|e| e.friendly(db.mode() == "free")));
        let _ = tx.send(Msg::Curriculum(course_id, r));
    });
}

pub fn on_curriculum(app: &mut App, cid: i64, r: Result<Value, String>) {
    app.busy = None;
    match r {
        Ok(data) => {
            if let Some(c) = Course::get(&app.db, cid) {
                ai::save_curriculum(&app.db, &c, &data);
            }
            let mut ev = game::award(&app.db, 10, 0, 0);
            game::unlock(&app.db, "pathfinder", &mut ev);
            app.sound.play("complete");
            app.confetti(90);
            app.handle(&ev, false);
            app.go(Route::Course(cid, Tab::Path));
        }
        Err(e) => app.error(&e),
    }
}

fn path(app: &mut App, ui: &mut Ui, c: &Course, concepts: &[Concept], no_materials: bool) {
    let p = app.pal;
    if concepts.is_empty() {
        card(&p).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                app.mascot(ui, 130.0, Mood::Happy);
                ui.label(RichText::new("Let's build your learning path").font(bold(22.0)));
                ui.label(RichText::new("Lumi reads your materials and lays out every concept from the basics to mastery, in order.").color(p.muted));
                ui.add_space(8.0);
                if widgets::button(ui, "✨ Generate my learning path", p.primary, app.dash.has_key).clicked() {
                    generate_path(app, c.id);
                }
                if no_materials {
                    ui.label(RichText::new("Tip: add materials first for a path based on your actual textbook.").size(13.0).color(p.faint));
                }
            });
        });
        return;
    }
    let mut units: Vec<(String, i64, Vec<&Concept>)> = vec![];
    for x in concepts {
        if units.last().map(|u| u.0 != x.unit_title).unwrap_or(true) {
            units.push((x.unit_title.clone(), x.unit_idx, vec![]));
        }
        units.last_mut().unwrap().2.push(x);
    }
    let current = concepts.iter().find(|x| x.level() < 3).map(|x| x.id);
    let beaten: Vec<i64> = app.db.query("SELECT unit_idx FROM bosses WHERE course_id=?", &[&c.id], |r| r.get(0));
    let width = ui.available_width().min(640.0);
    let left_pad = (ui.available_width() - width) / 2.0;
    let mut k = 0;
    let t = ui.input(|i| i.time) as f32;
    for (ui_idx, (title, unit_idx, items)) in units.iter().enumerate() {
        let color = hex(widgets::UNIT_COLORS[ui_idx % widgets::UNIT_COLORS.len()]);
        let prog = items.iter().map(|x| x.mastery).sum::<f64>() / items.len() as f64;
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(left_pad);
            egui::Frame::new().fill(color).corner_radius(20).inner_margin(egui::Margin::symmetric(22, 16)).show(ui, |ui| {
                ui.set_width(width - 44.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("UNIT {}", ui_idx + 1)).font(bold(12.0)).color(Color32::from_white_alpha(215)));
                        ui.label(RichText::new(title).font(bold(20.0)).color(Color32::WHITE));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(format!("{}%", prog as i64)).font(bold(22.0)).color(Color32::WHITE)));
                });
            });
        });
        ui.add_space(12.0);
        let n_nodes = items.len() + 1;
        for j in 0..n_nodes {
            let off = (k as f32 * 0.9).sin() * 120.0;
            k += 1;
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
            let center = egui::pos2(row.center().x + off, row.top() + 42.0);
            let node = egui::Rect::from_center_size(center, vec2(76.0, 70.0));
            let resp = ui.interact(node, ui.scope_id().with(("node", ui_idx, j)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let down = resp.is_pointer_button_down_on();
            let painter = ui.painter();
            let label_left = off > 0.0;
            if j == items.len() {
                let is_beaten = beaten.contains(unit_idx);
                let name = widgets::boss_name(c.id, ui_idx);
                let fill = if is_beaten { p.gold } else { hex("#2a1630") };
                painter.add(egui::Shape::ellipse_filled(node.center() + vec2(0.0, if down { 5.0 } else { 7.0 }), vec2(38.0, 35.0), widgets::darken(fill, 0.6)));
                painter.add(egui::Shape::ellipse_filled(node.center() + vec2(0.0, if down { 5.0 } else { 0.0 }), vec2(38.0, 35.0), fill));
                if is_beaten {
                    painter.text(node.center(), Align2::CENTER_CENTER, "🏆", egui::FontId::proportional(30.0), Color32::WHITE);
                } else {
                    widgets::boss(ui, node.shrink(10.0), color, BossMood::Idle);
                }
                let txt = if is_beaten { format!("{name} defeated ✓") } else { format!("⚔️ Boss: {name}") };
                let lp = if label_left { center - vec2(52.0, 0.0) } else { center + vec2(52.0, 0.0) };
                ui.painter().text(lp, if label_left { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER }, txt, bold(14.0), p.muted);
                if resp.clicked() {
                    app.dialog = Some(Dialog::BossIntro { course: c.id, unit: ui_idx });
                }
                continue;
            }
            let x = items[j];
            let lvl = x.level();
            let fill = match lvl { 0 => p.surface3, 1 => hex("#4b3cc4"), 2 => hex("#6a4dff"), 3 => hex("#c04fe0"), 4 => hex("#ff6b9a"), _ => p.gold };
            let is_cur = current == Some(x.id);
            if is_cur {
                let pulse = (t * 3.5).sin() * 0.5 + 0.5;
                painter.add(egui::Shape::ellipse_stroke(node.center(), vec2(48.0, 45.0), Stroke::new(6.0, Color32::from_rgba_unmultiplied(124, 92, 255, (40.0 + 60.0 * pulse) as u8))));
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
            painter.add(egui::Shape::ellipse_filled(node.center() + vec2(0.0, 7.0), vec2(38.0, 35.0), widgets::darken(fill, 0.6)));
            painter.add(egui::Shape::ellipse_filled(node.center() + vec2(0.0, if down { 5.0 } else { 0.0 }), vec2(38.0, 35.0), fill));
            let icon = if lvl >= 5 { "👑" } else if lvl >= 3 { "⭐" } else if x.attempts > 0 { "📖" } else { "✨" };
            painter.text(node.center() + vec2(0.0, if down { 5.0 } else { 0.0 }), Align2::CENTER_CENTER, icon, egui::FontId::proportional(30.0), Color32::WHITE);
            if lvl > 0 {
                let b = egui::Rect::from_center_size(node.right_bottom() - vec2(6.0, 2.0), vec2(36.0, 20.0));
                painter.rect(b, 10, p.gold, Stroke::new(3.0, p.bg), egui::StrokeKind::Outside);
                painter.text(b.center(), Align2::CENTER_CENTER, format!("👑{lvl}"), bold(11.0), Color32::from_rgb(58, 42, 0));
            }
            if is_cur {
                let bob = (t * 4.0).sin() * 4.0;
                let tag = egui::Rect::from_center_size(node.center_top() - vec2(0.0, 18.0 - bob), vec2(86.0, 24.0));
                painter.rect(tag, 10, p.surface, Stroke::new(2.0, p.line), egui::StrokeKind::Inside);
                painter.text(tag.center(), Align2::CENTER_CENTER, if x.attempts > 0 { "CONTINUE" } else { "START" }, bold(12.0), p.primary2);
            }
            let lp = if label_left { center - vec2(52.0, 0.0) } else { center + vec2(52.0, 0.0) };
            let lab = ui.painter().layout(x.title.clone(), bold(14.0), p.muted, 200.0);
            let pos = if label_left { lp - vec2(lab.size().x, lab.size().y / 2.0) } else { lp - vec2(0.0, lab.size().y / 2.0) };
            ui.painter().galley(pos, lab, p.muted);
            if resp.clicked() {
                app.sound.play("tap");
                app.dialog = Some(Dialog::ConceptMenu { course: c.id, concept: x.id });
            }
        }
    }
    ui.vertical_centered(|ui| {
        if ui.link(RichText::new("↻ Regenerate path (keeps progress on matching concepts)").color(p.primary2)).clicked() {
            generate_path(app, c.id);
        }
    });
}

pub fn concept_menu(app: &mut App, ctx: &egui::Context, course: i64, concept: i64) -> bool {
    let p = app.pal;
    let Some(x) = Concept::get(&app.db, concept) else { return false };
    let mut keep = true;
    modal_frame(ctx, &p, "concept", 380.0, |ui| {
        ui.label(RichText::new(&x.title).font(bold(20.0)));
        ui.label(RichText::new(&x.summary).color(p.muted));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mastery").size(13.0).color(p.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{} {}%", "👑".repeat(x.level() as usize), x.mastery as i64)).size(13.0));
            });
        });
        widgets::progress(ui, x.mastery as f32 / 100.0, p.primary, 10.0, ui.available_width());
        ui.add_space(12.0);
        let w = ui.available_width();
        if widgets::button_sized(ui, "📖 Learn with Lumi", p.primary, true, 15.0, w).clicked() {
            keep = false;
            let sid = actions::new_session(&app.db, course, Some(concept), None);
            app.go(Route::Learn(sid));
        }
        ui.add_space(6.0);
        if widgets::button_sized(ui, "⚡ Practice", p.green, true, 15.0, w).clicked() {
            keep = false;
            super::quiz::start(app, super::quiz::Request { course_id: Some(course), concept_ids: vec![concept], count: 8, ..Default::default() });
        }
        ui.add_space(6.0);
        if widgets::button_sized(ui, "❤️ Challenge mode · bonus XP", p.surface2, true, 13.0, w).clicked() {
            keep = false;
            super::quiz::start(app, super::quiz::Request { course_id: Some(course), concept_ids: vec![concept], count: 8, mode: "challenge".into(), ..Default::default() });
        }
        ui.add_space(6.0);
        if ui.link(RichText::new("Close").color(p.faint)).clicked() {
            keep = false;
        }
    });
    keep
}

pub fn boss_intro(app: &mut App, ctx: &egui::Context, course: i64, unit: usize) -> bool {
    let p = app.pal;
    let concepts = Concept::for_course(&app.db, course);
    let mut units: Vec<Vec<&Concept>> = vec![];
    for x in &concepts {
        if units.last().map(|u: &Vec<&Concept>| u[0].unit_title != x.unit_title).unwrap_or(true) {
            units.push(vec![]);
        }
        units.last_mut().unwrap().push(x);
    }
    let Some(items) = units.get(unit) else { return false };
    let color = hex(widgets::UNIT_COLORS[unit % widgets::UNIT_COLORS.len()]);
    let name = widgets::boss_name(course, unit);
    let mut keep = true;
    modal_frame(ctx, &p, "boss", 440.0, |ui| {
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(150.0, 150.0), Sense::hover());
            widgets::boss(ui, r, color, BossMood::Idle);
            ui.label(RichText::new(format!("BOSS BATTLE · UNIT {}", unit + 1)).font(bold(13.0)).color(p.muted));
            ui.label(RichText::new(name).font(bold(28.0)));
            ui.label(RichText::new(format!("Every right answer is an attack, and combos deal critical hits. Every mistake costs a heart. Lose all 3 and the boss wins. Defeat it to conquer “{}” and win 💎 gems.", items[0].unit_title)).color(p.muted));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(70.0);
                if widgets::button(ui, "Not yet", p.surface2, true).clicked() {
                    keep = false;
                }
                if widgets::button(ui, "⚔️ Fight!", p.red, true).clicked() {
                    keep = false;
                    let ids: Vec<i64> = items.iter().map(|x| x.id).collect();
                    let count = (ids.len() * 3).clamp(8, 12);
                    super::quiz::start(app, super::quiz::Request {
                        course_id: Some(course), concept_ids: ids, count, mode: "boss".into(), kind: Some("boss".into()),
                        boss: Some((name.to_string(), color, items[0].unit_idx)), ..Default::default()
                    });
                }
            });
        });
    });
    keep
}

// ---------- materials ----------

fn materials_tab(app: &mut App, ui: &mut Ui, c: &Course, mats: &[Material], no_path: bool) {
    let p = app.pal;
    let r = egui::Frame::new().fill(p.surface).stroke(Stroke::new(3.0, p.line)).corner_radius(22).inner_margin(30).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("📥").size(44.0));
            ui.label(RichText::new("Drop your study materials here").font(bold(21.0)));
            ui.label(RichText::new("Textbooks, lecture notes, past quizzes, problem sets, slides as PDF, Word docs, text, photos of your notes…").color(p.muted));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 320.0).max(0.0) / 2.0);
                if widgets::button(ui, "Choose files", p.primary, true).clicked() {
                    app.pick_files(Pick::Materials(c.id), false);
                }
                if widgets::button(ui, "Paste text", p.surface2, true).clicked() {
                    app.dialog = Some(Dialog::Note { course: c.id, title: String::new(), text: String::new() });
                }
            });
        });
    });
    let _ = r;
    let total_pages: i64 = mats.iter().filter(|m| m.enabled && m.kind == "pdf" && !materials::is_large(m)).map(|m| m.pages).sum();
    if total_pages > 500 {
        ui.add_space(10.0);
        ui.label(RichText::new(format!("⚠️ {total_pages} pages of smaller PDFs are switched on and get sent whole. Switch off the ones you're not studying to keep lessons fast.")).color(p.flame));
    }
    ui.add_space(8.0);
    let mut toggle: Option<(i64, bool)> = None;
    let mut delete: Option<i64> = None;
    for m in mats {
        card(&p).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(match m.kind.as_str() { "pdf" => "📕", "image" => "🖼️", _ => "📝" }).size(26.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(&m.filename).font(bold(15.0)));
                    let big = materials::is_large(m);
                    let status = if m.kind == "image" { String::new() } else if !m.indexed { " · indexing…".into() } else if big { " · big book: Lumi pulls up the relevant pages".into() } else { String::new() };
                    ui.label(RichText::new(format!("{}{:.1} MB{status}", if m.pages > 0 && m.kind == "pdf" { format!("{} pages · ", m.pages) } else { String::new() }, m.size as f64 / 1048576.0)).size(13.0).color(p.muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.link(RichText::new("✕").color(p.faint)).on_hover_text("Delete").clicked() {
                        delete = Some(m.id);
                    }
                    if toggle_switch(ui, &p, m.enabled).on_hover_text("Use in lessons and quizzes").clicked() {
                        toggle = Some((m.id, !m.enabled));
                    }
                    ui.label(RichText::new(if m.enabled { "Used by Lumi" } else { "Off" }).size(13.0).color(p.muted));
                });
            });
        });
        ui.add_space(8.0);
        if !m.indexed && m.kind != "image" {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(1000));
        }
    }
    if let Some((id, en)) = toggle {
        app.db.exec("UPDATE materials SET enabled=? WHERE id=?", &[&(en as i64), &id]);
    }
    if let Some(id) = delete {
        materials::delete(&app.db, id);
    }
    if !mats.is_empty() && no_path {
        ui.add_space(14.0);
        ui.vertical_centered(|ui| {
            if widgets::button(ui, "✨ Generate my learning path", p.green, app.dash.has_key).clicked() {
                generate_path(app, c.id);
            }
        });
    }
    ui.add_space(10.0);
    ui.label(RichText::new("Materials are sent to the AI when you study. Big textbooks are indexed once, then only the relevant pages are sent. New lessons use whatever is switched on when they start.").size(13.0).color(p.faint));
}

pub fn toggle_switch(ui: &mut Ui, p: &super::theme::Pal, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(46.0, 26.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    ui.painter().rect_filled(r, 13, if on { p.green } else { p.surface3 });
    let x = if on { r.right() - 13.0 } else { r.left() + 13.0 };
    ui.painter().circle_filled(egui::pos2(x, r.center().y), 10.0, Color32::WHITE);
    resp
}

pub fn upload(app: &mut App, cid: i64, files: Vec<std::path::PathBuf>) {
    app.toast("📥", "Adding materials…", &format!("{} file(s)", files.len()), 1);
    app.spawn(move |db, tx| {
        let mut added = vec![];
        let mut errs = vec![];
        for f in &files {
            match materials::add_file(db, cid, f) {
                Ok(id) => added.push(id),
                Err(e) => errs.push(format!("{}: {e}", f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())),
            }
        }
        let _ = tx.send(Msg::Uploaded(cid, added.len(), errs));
        for id in added {
            materials::ensure_ready(db, id); // page text, search index, table of contents
        }
    });
}

pub fn on_uploaded(app: &mut App, _cid: i64, n: usize, errs: Vec<String>) {
    for e in errs {
        app.toast("⚠️", "Skipped a file", &e, 2);
    }
    if n > 0 {
        app.sound.play("correct");
        app.toast("📚", &format!("Added {n} material{}", if n > 1 { "s" } else { "" }), "Lumi will use them in lessons and quizzes.", 1);
        let mut ev = game::Events::default();
        if app.db.int("SELECT COUNT(*) FROM materials", &[]) >= 3 {
            game::unlock(&app.db, "librarian", &mut ev);
        }
        app.handle(&ev, false);
    }
}

fn lessons(app: &mut App, ui: &mut Ui, c: &Course) {
    let p = app.pal;
    card_glow(&p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Open study session").font(bold(17.0)));
                ui.label(RichText::new("Stuck on a homework problem? Want a chapter explained? Ask Lumi anything about this course.").color(p.muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::button(ui, "💬 Start session", p.primary, true).clicked() {
                    let sid = actions::new_session(&app.db, c.id, None, None);
                    app.go(Route::Learn(sid));
                }
            });
        });
    });
    super::section(ui, &p, "Past lessons");
    let list = Session::list(&app.db, c.id);
    if list.is_empty() {
        ui.label(RichText::new("No lessons yet. Pick a concept on your path and tap “Learn with Lumi”.").color(p.muted));
    }
    for (sid, title, concept, updated) in list {
        let r = card(&p).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(if concept.is_some() { "📖" } else { "💬" }).size(24.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).font(bold(15.0)));
                    ui.label(RichText::new(updated).size(12.0).color(p.faint));
                });
            });
        });
        if r.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.go(Route::Learn(sid));
        }
        ui.add_space(6.0);
    }
}

// ---------- dialogs ----------

const EMOJIS: [&str; 16] = ["📘", "🧮", "🧪", "🧬", "⚛️", "🌍", "🏛️", "💻", "📈", "🎨", "🎵", "🗣️", "⚖️", "🩺", "🔭", "✍️"];

pub fn new_course_dialog(app: &mut App, ctx: &egui::Context, name: &mut String, goal: &mut String, emoji: &mut String) -> bool {
    let p = app.pal;
    let mut keep = true;
    modal_frame(ctx, &p, "newcourse", 500.0, |ui| {
        ui.label(RichText::new("New course").font(bold(28.0)));
        ui.label(RichText::new("What do you want to master?").color(p.muted));
        ui.add_space(10.0);
        ui.label(RichText::new("Course name").font(bold(14.0)).color(p.muted));
        let r = ui.add(egui::TextEdit::singleline(name).hint_text("e.g. Organic Chemistry, Calculus II, AP US History").desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
        if name.is_empty() && !r.has_focus() {
            r.request_focus();
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Your goal (optional)").font(bold(14.0)).color(p.muted));
        ui.add(egui::TextEdit::singleline(goal).hint_text("e.g. Ace the midterm, understand it deeply").desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
        ui.add_space(8.0);
        ui.label(RichText::new("Icon").font(bold(14.0)).color(p.muted));
        ui.horizontal_wrapped(|ui| {
            for e in EMOJIS {
                let sel = emoji == e;
                if ui.add(egui::Button::new(RichText::new(e).size(22.0)).fill(if sel { widgets::lerp(p.surface2, p.primary, 0.4) } else { p.surface2 })).clicked() {
                    *emoji = e.to_string();
                }
            }
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, "Cancel", p.surface2, true).clicked() {
                keep = false;
            }
            if widgets::button(ui, "Create", p.primary, !name.trim().is_empty()).clicked() {
                let id = actions::new_course(&app.db, name, goal, emoji);
                keep = false;
                app.go(Route::Course(id, Tab::Materials));
            }
        });
    });
    keep
}

pub fn note_dialog(app: &mut App, ctx: &egui::Context, course: i64, title: &mut String, text: &mut String) -> bool {
    let p = app.pal;
    let mut keep = true;
    modal_frame(ctx, &p, "note", 560.0, |ui| {
        ui.label(RichText::new("Paste text").font(bold(26.0)));
        ui.add(egui::TextEdit::singleline(title).hint_text("Title, e.g. Lecture 5 notes").desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
        ui.add_space(6.0);
        ui.add(egui::TextEdit::multiline(text).hint_text("Paste notes, a problem set, quiz questions…").desired_rows(10).desired_width(f32::INFINITY));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, "Cancel", p.surface2, true).clicked() {
                keep = false;
            }
            if widgets::button(ui, "Add", p.primary, !text.trim().is_empty()).clicked() {
                let id = materials::add_note(&app.db, course, title, text);
                let db = app.db.clone();
                std::thread::spawn(move || materials::ensure_ready(&db, id));
                keep = false;
                on_uploaded(app, course, 1, vec![]);
            }
        });
    });
    keep
}
