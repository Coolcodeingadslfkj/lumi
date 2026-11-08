//! Home: greeting, daily goal, quests + chest, Lumi's evolution, charts, courses.

use super::widgets::{self, bold, card, card_glow, Mood};
use super::{column_grid, App, Dialog, Route, Tab};
use crate::game::{self, Chest};
use crate::model::{Concept, Course};
use egui::{vec2, Align, Align2, Color32, Layout, RichText, Sense, Ui};

fn greeting(app: &App) -> String {
    let d = &app.dash;
    let n = if d.name.is_empty() { String::new() } else { format!(", {}", d.name) };
    let h = chrono::Timelike::hour(&chrono::Local::now());
    let hello = if h < 12 { "Good morning" } else if h < 18 { "Good afternoon" } else { "Good evening" };
    if !d.has_key {
        return format!("Hi{n}! I'm Lumi, your personal tutor. Add a free AI key in Settings and I'll wake right up. ✨");
    }
    if d.today_xp >= d.goal {
        return format!("Goal smashed today{n}! Anything extra now is pure bonus brainpower. 🧠");
    }
    if d.streak >= 3 && d.today_xp == 0 {
        return format!("{hello}{n}! You're on a {}-day streak 🔥 Let's keep it burning.", d.streak);
    }
    if d.due > 0 {
        return format!("{hello}{n}! {} concept{} ready for review: a quick round locks them into long-term memory.", d.due, if d.due > 1 { "s are" } else { " is" });
    }
    format!("{hello}{n}! Ready to learn something that sticks?")
}

/// The next concept worth studying: the most recent course's first concept below 3 crowns.
pub fn next_concept(app: &App) -> Option<(i64, i64)> {
    for c in Course::all(&app.db).iter().rev() {
        if let Some(x) = Concept::for_course(&app.db, c.id).into_iter().find(|x| x.level() < 3) {
            return Some((c.id, x.id));
        }
    }
    None
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let d = app.dash.clone();
    card_glow(&p).show(ui, |ui| {
        ui.horizontal(|ui| {
            app.mascot(ui, 120.0, if d.today_xp >= d.goal { Mood::Celebrate } else { Mood::Happy });
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() - 140.0);
                egui::Frame::new().fill(p.surface2).corner_radius(18).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
                    ui.label(RichText::new(greeting(app)).size(17.0));
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if !d.has_key {
                        if widgets::button(ui, "Add an AI key", p.primary, true).clicked() {
                            app.go(Route::Settings);
                        }
                    } else if Course::all(&app.db).is_empty() {
                        if widgets::button(ui, "Create your first course", p.primary, true).clicked() {
                            app.dialog = Some(Dialog::NewCourse { name: String::new(), goal: String::new(), emoji: "📘".into() });
                        }
                    } else {
                        if widgets::button(ui, "Continue learning", p.green, true).clicked() {
                            match next_concept(app) {
                                Some((course, concept)) => app.dialog = Some(Dialog::ConceptMenu { course, concept }),
                                None => app.go(Route::Courses),
                            }
                        }
                        if d.due > 0 && widgets::button(ui, &format!("Review {} due", d.due), p.surface2, true).clicked() {
                            app.go(Route::Review);
                        }
                    }
                });
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let f = d.today_xp as f32 / d.goal.max(1) as f32;
                widgets::ring(ui, &p, 118.0, f, &if f >= 1.0 { "✓".into() } else { d.today_xp.to_string() },
                    &if f >= 1.0 { "goal hit".into() } else { format!("/ {} XP", d.goal) }, if f >= 1.0 { p.gold } else { p.green });
            });
        });
    });
    // next upcoming test
    let upcoming: Vec<_> = crate::model::Test::all(&app.db).into_iter().filter(|t| crate::testprep::days_left(t).map(|x| x >= 0).unwrap_or(true)).collect();
    if let Some(t) = upcoming.first() {
        ui.add_space(14.0);
        let rows = crate::testprep::concepts(&app.db, t.id);
        let ready = crate::testprep::readiness(&rows);
        let cd = super::tests::countdown(crate::testprep::days_left(t));
        let r = egui::Frame::new().fill(widgets::lerp(p.surface, p.green, 0.12)).stroke(egui::Stroke::new(2.0, widgets::lerp(p.line, p.green, 0.5))).corner_radius(18).inner_margin(16).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("📝").size(32.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{} · {}", t.name, if cd.0 == "Today" { "today!".to_string() } else { format!("{} {}", cd.0, cd.1) })).font(bold(16.0)));
                    widgets::progress(ui, ready as f32 / 100.0, if ready >= 80 { p.green } else { p.primary }, 8.0, 360.0);
                    ui.label(RichText::new(format!("{ready}% ready{}", if upcoming.len() > 1 { format!(" · +{} more", upcoming.len() - 1) } else { String::new() })).size(13.0).color(p.muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button_sized(ui, "Continue prep →", p.green, true, 13.0, 0.0).clicked() {
                        app.go(Route::Test(t.id));
                    }
                });
            });
        });
        if r.response.interact(Sense::click()).clicked() {
            app.go(Route::Test(t.id));
        }
    }
    ui.add_space(14.0);
    let acc = if d.today_answers > 0 { 100 * d.today_correct / d.today_answers } else { 0 };
    let stats = [
        ("🔥", d.streak.to_string(), "day streak".to_string(), None),
        ("⚡", format!("Level {}", d.level.0), format!("{} XP to next", d.level.2 - d.level.1), Some(d.level.1 as f32 / d.level.2.max(1) as f32)),
        ("🎯", format!("{acc}%"), "accuracy today".into(), None),
        ("🏆", format!("{}/{}", d.achievements, game::ACHIEVEMENTS.len()), "achievements".into(), None),
    ];
    column_grid(ui, 4, 14.0, stats.len(), |ui, i, w| {
        let (icon, big, small, bar) = &stats[i];
        card(&p).inner_margin(14).show(ui, |ui| {
            ui.set_width(w - 32.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(*icon).size(28.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(big).font(bold(21.0)));
                    if let Some(f) = bar {
                        widgets::progress(ui, *f, p.gold, 7.0, (w - 110.0).max(60.0));
                    }
                    ui.label(RichText::new(small).size(13.0).color(p.muted));
                });
            });
        });
    });
    ui.add_space(14.0);
    column_grid(ui, 2, 16.0, 2, |ui, i, w| {
        card(&p).show(ui, |ui| {
            ui.set_width(w - 40.0);
            ui.set_min_height(170.0);
            if i == 0 {
                quests(app, ui);
            } else {
                lumi_card(app, ui);
            }
        });
    });
    ui.add_space(14.0);
    column_grid(ui, 2, 16.0, 2, |ui, i, w| {
        card(&p).show(ui, |ui| {
            ui.set_width(w - 40.0);
            if i == 0 {
                ui.label(RichText::new("XP this week").font(bold(16.0)));
                week_chart(app, ui, w - 40.0);
            } else {
                ui.label(RichText::new("Your study streak map").font(bold(16.0)));
                heatmap(app, ui);
            }
        });
    });
    super::section(ui, &p, "Your courses");
    super::courses::cards(app, ui);
}

fn quests(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let (qs, chest) = game::quests_state(&app.db);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("📜 Daily quests").font(bold(16.0)));
            ui.label(RichText::new("New quests every day. Finish all three to open the chest.").size(13.0).color(p.muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(72.0, 72.0), Sense::click());
            let t = ui.input(|i| i.time) as f32;
            let wob = if chest == Chest::Ready { (t * 6.0).sin() * 0.15 } else { 0.0 };
            ui.painter().rect_filled(rect, 18, if chest == Chest::Ready { widgets::lerp(p.surface2, p.gold, 0.35) } else { p.surface2 });
            let icon = match chest { Chest::Locked => "🔒", Chest::Ready => "🎁", Chest::Claimed => "✅" };
            let center = rect.center() + vec2(wob * 20.0, 0.0);
            ui.painter().text(center, Align2::CENTER_CENTER, icon, egui::FontId::proportional(if chest == Chest::Ready { 40.0 } else { 32.0 }), Color32::WHITE);
            let resp = resp.on_hover_text(match chest { Chest::Ready => "Open your chest!", Chest::Claimed => "Opened today. Come back tomorrow!", Chest::Locked => "Finish all quests to unlock" });
            if chest == Chest::Ready {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
                if resp.clicked() {
                    if let Some(ev) = game::claim_chest(&app.db) {
                        app.sound.play("achievement");
                        let now = ui.input(|i| i.time);
                        app.dialog = Some(Dialog::Chest { gems: ev.chest_gems, freeze: ev.chest_freeze, opened_at: now });
                        app.handle(&ev, false);
                    }
                }
            }
        });
    });
    for q in qs {
        let done = q.value >= q.target;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} {}", if done { "✅" } else { "▫️" }, q.text)).color(if done { p.green } else { p.text }));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(format!("{}/{}", q.value, q.target)).size(13.0).color(p.muted)));
        });
        widgets::progress(ui, q.value as f32 / q.target.max(1) as f32, if done { p.green } else { p.gold }, 8.0, ui.available_width());
    }
}

pub fn chest_dialog(app: &mut App, ctx: &egui::Context, gems: i64, freeze: bool, opened_at: f64) -> bool {
    let p = app.pal;
    let now = ctx.input(|i| i.time);
    let opened = now - opened_at > 0.9;
    let mut keep = true;
    super::modal_frame(ctx, &p, "chest", 420.0, |ui| {
        ui.vertical_centered(|ui| {
            let t = now as f32;
            let wob = if opened { 0.0 } else { (t * 20.0).sin() * 10.0 };
            let (rect, _) = ui.allocate_exact_size(vec2(140.0, 120.0), Sense::hover());
            ui.painter().text(rect.center() + vec2(wob, 0.0), Align2::CENTER_CENTER, if opened { "💎" } else { "🎁" }, egui::FontId::proportional(90.0), Color32::WHITE);
            if opened {
                ui.label(RichText::new("Quest chest opened!").font(bold(26.0)));
                ui.label(RichText::new(format!("+{gems} 💎")).font(bold(54.0)).color(p.ice));
                ui.label(RichText::new(format!("+15 XP{}", if freeze { " · +1 streak freeze 🧊 (lucky!)" } else { "" })).color(p.muted));
                ui.add_space(8.0);
                if widgets::button_sized(ui, "Collect", p.gold, true, 15.0, 300.0).clicked() {
                    keep = false;
                }
            } else {
                ui.label(RichText::new("Opening…").font(bold(26.0)));
                ctx.request_repaint();
            }
        });
    });
    if opened && ctx.data_mut(|d| !std::mem::replace(d.get_temp_mut_or_default::<bool>(egui::Id::unique("chest_burst")), true)) {
        app.sound.play("complete");
        app.confetti(180);
    }
    if !keep {
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("chest_burst"), false));
    }
    keep
}

fn lumi_card(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let d = app.dash.clone();
    ui.horizontal(|ui| {
        app.mascot(ui, 110.0, Mood::Happy);
        ui.vertical(|ui| {
            ui.label(RichText::new("YOUR LUMI").font(bold(12.0)).color(p.muted));
            if let Some(s) = &d.stage {
                ui.label(RichText::new(format!("{} stage", s.name)).font(bold(18.0)));
                match (s.next_name, s.next_level) {
                    (Some(n), Some(l)) => {
                        ui.label(RichText::new(format!("Evolves into {n} at level {l}")).size(13.0).color(p.muted));
                        widgets::progress(ui, (d.level.0 - 1) as f32 / (l - 1).max(1) as f32, p.gold, 8.0, ui.available_width().min(260.0));
                    }
                    _ => {
                        ui.label(RichText::new("Fully evolved. Legendary! 🌟").size(13.0).color(p.muted));
                    }
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if widgets::button_sized(ui, &format!("💎 {} · Shop", d.gems), p.primary, true, 12.0, 0.0).clicked() {
                    app.go(Route::Shop);
                }
                if d.mistakes > 0 && widgets::button_sized(ui, &format!("📓 {} to fix", d.mistakes), p.surface2, true, 12.0, 0.0).clicked() {
                    app.go(Route::Mistakes(None));
                }
            });
        });
    });
}

fn week_chart(app: &App, ui: &mut Ui, width: f32) {
    let p = app.pal;
    let data = game::week(&app.db);
    let goal = app.dash.goal as f32;
    let h = 170.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    let (pt, pb) = (22.0, 24.0);
    let max = data.iter().map(|d| d.xp as f32).fold(goal * 1.15, f32::max).max(1.0);
    let y = |v: f32| rect.top() + pt + (h - pt - pb) * (1.0 - v / max);
    let painter = ui.painter();
    painter.line_segment([egui::pos2(rect.left(), y(0.0)), egui::pos2(rect.right(), y(0.0))], egui::Stroke::new(1.0, p.line));
    let gy = y(goal);
    let mut x = rect.left();
    while x < rect.right() {
        painter.line_segment([egui::pos2(x, gy), egui::pos2((x + 4.0).min(rect.right()), gy)], egui::Stroke::new(1.5, p.gold));
        x += 8.0;
    }
    painter.text(egui::pos2(rect.left(), gy - 5.0), Align2::LEFT_BOTTOM, format!("daily goal {}", goal as i64), bold(11.0), p.gold);
    let bw = width / data.len() as f32;
    let bar = (bw * 0.45).min(26.0);
    for (i, d) in data.iter().enumerate() {
        let cx = rect.left() + bw * i as f32 + bw / 2.0;
        let today = i == data.len() - 1;
        if d.xp > 0 {
            let r = egui::Rect::from_min_max(egui::pos2(cx - bar / 2.0, y(d.xp as f32)), egui::pos2(cx + bar / 2.0, y(0.0)));
            painter.rect_filled(r, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, p.primary);
            if today {
                painter.text(egui::pos2(cx, r.top() - 6.0), Align2::CENTER_BOTTOM, d.xp.to_string(), bold(11.0), p.text);
            }
        }
        let label = if today { "Today".to_string() } else { game::date_of(&d.day).map(|x| x.format("%a").to_string()).unwrap_or_default() };
        painter.text(egui::pos2(cx, rect.bottom() - 6.0), Align2::CENTER_BOTTOM, label, bold(11.0), if today { p.text } else { p.faint });
        let hit = egui::Rect::from_min_max(egui::pos2(cx - bw / 2.0, rect.top() + pt), egui::pos2(cx + bw / 2.0, y(0.0)));
        let day = game::date_of(&d.day).map(|x| x.format("%a, %b %-d").to_string()).unwrap_or_default();
        ui.interact(hit, ui.scope_id().with(("wk", i)), Sense::hover()).on_hover_text(format!("{day} · {} XP{}", d.xp, if d.xp as f32 >= goal { " 🎯" } else { "" }));
    }
}

fn heatmap(app: &App, ui: &mut Ui) {
    use chrono::Duration;
    let p = app.pal;
    let (start, days) = game::heatmap(&app.db);
    let goal = app.dash.goal;
    let ramp = [p.surface2, widgets::hex("#2f2770"), widgets::hex("#4a39b0"), widgets::hex("#6d52f0"), widgets::hex("#a28cff"), widgets::hex("#d6ccff")];
    let freeze_c = widgets::hex("#2b6f8f");
    let today = chrono::Local::now().date_naive();
    let cell = 15.0;
    let gap = 3.0;
    let weeks = ((today - start).num_days() / 7 + 1) as usize;
    let (rect, _) = ui.allocate_exact_size(vec2(weeks as f32 * (cell + gap), 7.0 * (cell + gap)), Sense::hover());
    for w in 0..weeks {
        for dow in 0..7 {
            let date = start + Duration::days((w * 7 + dow) as i64);
            if date > today {
                continue;
            }
            let key = date.format("%Y-%m-%d").to_string();
            let rec = days.get(&key);
            let xp = rec.map(|r| r.xp).unwrap_or(0);
            let frozen = rec.map(|r| r.frozen).unwrap_or(false) && xp == 0;
            let lvl = if xp <= 0 { 0 } else if xp < goal / 2 { 1 } else if xp < goal { 2 } else if xp < goal * 2 { 3 } else if xp < goal * 3 { 4 } else { 5 };
            let r = egui::Rect::from_min_size(rect.min + vec2(w as f32 * (cell + gap), dow as f32 * (cell + gap)), vec2(cell, cell));
            ui.painter().rect_filled(r, 3, if frozen { freeze_c } else { ramp[lvl] });
            ui.interact(r, ui.scope_id().with(("hm", w, dow)), Sense::hover())
                .on_hover_text(format!("{} · {}", date.format("%a, %b %-d"), if frozen { "🧊 streak freeze".to_string() } else { format!("{xp} XP") }));
        }
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Less").size(11.0).color(p.faint));
        for c in ramp {
            let (r, _) = ui.allocate_exact_size(vec2(11.0, 11.0), Sense::hover());
            ui.painter().rect_filled(r, 2, c);
        }
        ui.label(RichText::new("More ·").size(11.0).color(p.faint));
        let (r, _) = ui.allocate_exact_size(vec2(11.0, 11.0), Sense::hover());
        ui.painter().rect_filled(r, 2, freeze_c);
        ui.label(RichText::new("freeze").size(11.0).color(p.faint));
    });
}

#[allow(dead_code)]
fn _tab() -> Tab {
    Tab::Path
}
