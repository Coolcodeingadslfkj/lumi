//! Review, mistake notebook, achievements, shop, settings, welcome.

use super::widgets::{self, bold, card, card_glow, hex, Mood};
use super::{column_grid, modal_frame, App, Msg, Route};
use crate::model::{Course, Mistake};
use crate::{game, llm, shop};
use egui::{vec2, Align, Color32, Layout, RichText, Sense, Stroke, Ui};

pub fn review(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    super::heading(ui, &p, "Spaced review 🧠", "Lumi schedules each concept to come back right before you'd forget it. Short reviews now mean you won't have to relearn it later.");
    let courses: Vec<(Course, usize)> = Course::all(&app.db).into_iter().map(|c| { let n = game::due_concepts(&app.db, Some(c.id)).len(); (c, n) }).filter(|x| x.1 > 0).collect();
    if courses.is_empty() {
        card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                app.mascot(ui, 130.0, Mood::Celebrate);
                ui.label(RichText::new("All caught up!").font(bold(22.0)));
                ui.label(RichText::new("Nothing is due right now. Practice new concepts and they'll show up here when it's time to refresh them.").color(p.muted));
                if let Some((course, _)) = super::home::next_concept(app) {
                    ui.add_space(6.0);
                    if widgets::button(ui, "Practice anyway", p.primary, true).clicked() {
                        super::quiz::start(app, super::quiz::Request { course_id: Some(course), ..Default::default() });
                    }
                }
            });
        });
        return;
    }
    column_grid(ui, 3, 16.0, courses.len(), |ui, i, w| {
        let (c, n) = &courses[i];
        card(&p).show(ui, |ui| {
            ui.set_width(w - 40.0);
            ui.label(RichText::new(&c.emoji).size(34.0));
            ui.label(RichText::new(&c.name).font(bold(17.0)));
            ui.label(RichText::new(format!("{n} concept{} ready for review", if *n > 1 { "s" } else { "" })).color(p.muted));
            if widgets::button_sized(ui, "Start review", p.green, true, 14.0, w - 40.0).clicked() {
                super::quiz::start(app, super::quiz::Request { course_id: Some(c.id), review: true, ..Default::default() });
            }
        });
    });
}

pub fn mistakes(app: &mut App, ui: &mut Ui, filter: Option<i64>) {
    let p = app.pal;
    super::heading(ui, &p, "Mistake notebook 📓", "Every question you miss lands here. “Fix” sessions give you a fresh version of each one, aimed at the exact misconception. Get it right and it's crossed off.");
    let items = Mistake::unfixed(&app.db, filter, 200);
    let fixed_total = app.db.int("SELECT COUNT(*) FROM mistakes WHERE fixed=1", &[]);
    let courses = Course::all(&app.db);
    ui.horizontal_wrapped(|ui| {
        widgets::chip(ui, &p, &format!("📌 {} to fix", items.len()), p.pink);
        widgets::chip(ui, &p, &format!("✅ {fixed_total} fixed all-time"), p.green);
        if courses.len() > 1 {
            if ui.selectable_label(filter.is_none(), "All courses").clicked() {
                app.go(Route::Mistakes(None));
            }
            for c in &courses {
                if ui.selectable_label(filter == Some(c.id), format!("{} {}", c.emoji, c.name)).clicked() {
                    app.go(Route::Mistakes(Some(c.id)));
                }
            }
        }
    });
    ui.add_space(12.0);
    let mut groups: Vec<(i64, usize)> = vec![];
    for m in &items {
        match groups.iter_mut().find(|g| g.0 == m.course_id) {
            Some(g) => g.1 += 1,
            None => groups.push((m.course_id, 1)),
        }
    }
    if !groups.is_empty() {
        column_grid(ui, 3, 16.0, groups.len(), |ui, i, w| {
            let (cid, n) = groups[i];
            let c = Course::get(&app.db, cid).unwrap_or_default();
            card_glow(&p).show(ui, |ui| {
                ui.set_width(w - 40.0);
                ui.label(RichText::new(format!("{} {}", c.emoji, c.name)).font(bold(16.0)));
                ui.label(RichText::new(format!("{n} mistake{} waiting", if n > 1 { "s" } else { "" })).color(p.muted));
                if widgets::button_sized(ui, "🔧 Fix my mistakes", p.green, app.dash.has_key, 14.0, w - 40.0).clicked() {
                    super::quiz::start(app, super::quiz::Request { course_id: Some(cid), kind: Some("mistakes".into()), count: 8, return_to: Some(Route::Mistakes(filter)), ..Default::default() });
                }
            });
        });
        ui.add_space(12.0);
    } else {
        card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                app.mascot(ui, 120.0, Mood::Celebrate);
                ui.label(RichText::new("No mistakes to fix!").font(bold(22.0)));
                ui.label(RichText::new("Missed questions from practice and lessons will show up here.").color(p.muted));
            });
        });
    }
    let mut dismiss = None;
    for m in &items {
        card(&p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(c) = m.concept_id.and_then(|c| crate::model::Concept::get(&app.db, c)) {
                    widgets::tag(ui, &c.title, p.muted, p.surface2);
                }
                if m.times_missed > 1 {
                    widgets::tag(ui, &format!("missed {}×", m.times_missed), p.pink, widgets::lerp(p.surface, p.pink, 0.18));
                }
                ui.label(RichText::new(m.updated_at.get(..10).unwrap_or("")).size(12.0).color(p.faint));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.link(RichText::new("✕").color(p.faint)).on_hover_text("Remove from notebook").clicked() {
                        dismiss = Some(m.id);
                    }
                });
            });
            ui.add_space(4.0);
            super::md::show(ui, &mut app.md_cache, m.question["prompt"].as_str().unwrap_or(""));
            ui.label(RichText::new(format!("✗ You: {}", if m.user_answer.is_empty() { "(skipped)" } else { &m.user_answer })).color(p.red));
            ui.label(RichText::new(format!("✓ Answer: {}", super::md::math_to_unicode(&super::quiz::answer_text(&m.question)))).color(p.green));
            let e = m.question["explanation"].as_str().unwrap_or("");
            if !e.is_empty() {
                ui.label(RichText::new(super::md::math_to_unicode(e)).size(13.5).color(p.muted));
            }
        });
        ui.add_space(10.0);
    }
    if let Some(id) = dismiss {
        app.db.exec("DELETE FROM mistakes WHERE id=?", &[&id]);
        app.refresh();
    }
}

pub fn achievements(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let got: std::collections::HashMap<String, String> = app.db.query("SELECT key, unlocked_at FROM achievements", &[], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default()))).into_iter().collect();
    super::heading(ui, &p, "Achievements", &format!("{} of {} unlocked", got.len().min(game::ACHIEVEMENTS.len()), game::ACHIEVEMENTS.len()));
    widgets::progress(ui, got.len() as f32 / game::ACHIEVEMENTS.len() as f32, p.gold, 12.0, 360.0);
    ui.add_space(16.0);
    column_grid(ui, 4, 16.0, game::ACHIEVEMENTS.len(), |ui, i, w| {
        let a = &game::ACHIEVEMENTS[i];
        let unlocked = got.get(a.key);
        card(&p).show(ui, |ui| {
            ui.set_width(w - 40.0);
            ui.set_min_height(150.0);
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(80.0, 80.0), Sense::hover());
                let fill = if unlocked.is_some() { p.gold } else { p.surface3 };
                ui.painter().circle_filled(r.center() + vec2(0.0, 5.0), 40.0, widgets::darken(fill, 0.6));
                ui.painter().circle_filled(r.center(), 40.0, fill);
                let icon_color = if unlocked.is_some() { Color32::WHITE } else { Color32::from_white_alpha(70) };
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, a.icon, egui::FontId::proportional(40.0), icon_color);
                ui.label(RichText::new(a.name).font(bold(15.0)).color(if unlocked.is_some() { p.text } else { p.faint }));
                ui.label(RichText::new(a.desc).size(13.0).color(p.muted));
                if let Some(d) = unlocked {
                    ui.label(RichText::new(d.get(..10).unwrap_or("")).size(12.0).color(p.faint));
                }
            });
        });
    });
}

pub fn shop(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let gems = game::gems(&app.db);
    let owned = game::owned(&app.db);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("Shop 💎").font(bold(32.0)));
            ui.label(RichText::new("Earn gems from daily quest chests, boss battles, perfect sessions, level-ups and daily goals.").color(p.muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| widgets::chip(ui, &p, &format!("💎 {gems}"), p.ice));
    });
    ui.add_space(14.0);
    card_glow(&p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            app.mascot(ui, 140.0, Mood::Happy);
            ui.vertical(|ui| {
                ui.label(RichText::new("Dress up Lumi").font(bold(22.0)));
                ui.label(RichText::new("Hats show up everywhere Lumi does: lessons, quizzes, celebrations.").color(p.muted));
            });
        });
    });
    let mut action: Option<(&str, String)> = None;
    for (kind, title) in [("hat", "🎩 Hats"), ("theme", "🎨 Themes"), ("powerup", "⚡ Power-ups")] {
        super::section(ui, &p, title);
        let mut items: Vec<Option<&shop::Item>> = shop::items().iter().filter(|i| i.kind == kind).map(Some).collect();
        if kind != "powerup" {
            items.insert(0, None); // "no hat" / default theme
        }
        let equipped = if kind == "hat" { app.dash.hat.clone() } else { app.dash.theme.clone() };
        column_grid(ui, 5, 14.0, items.len(), |ui, i, w| {
            let it = items[i];
            let is_eq = match it { Some(x) => equipped.as_deref() == Some(x.id.as_str()), None => equipped.is_none() };
            egui::Frame::new().fill(p.surface).stroke(Stroke::new(2.0, if is_eq { p.green } else { p.line })).corner_radius(18).inner_margin(14).show(ui, |ui| {
                ui.set_width(w - 32.0);
                ui.vertical_centered(|ui| {
                    match (kind, it) {
                        ("hat", _) => {
                            widgets::mascot_widget(ui, 100.0, if it.is_some() { Mood::Happy } else { Mood::Idle }, &app.dash.stage_key, it.map(|x| x.id.as_str()));
                        }
                        ("theme", _) => {
                            let pal = super::theme::palette(it.map(|x| x.id.as_str()));
                            let accent = it.and_then(|x| x.colors.get("accent")).map(|c| hex(c)).unwrap_or(pal.pink);
                            let (r, _) = ui.allocate_exact_size(vec2(w - 40.0, 64.0), Sense::hover());
                            let third = r.width() / 3.0;
                            for (k, c) in [pal.bg, pal.primary, accent].iter().enumerate() {
                                ui.painter().rect_filled(egui::Rect::from_min_size(r.min + vec2(third * k as f32, 0.0), vec2(third, r.height())), 0, *c);
                            }
                        }
                        _ => {
                            ui.label(RichText::new(it.map(|x| x.icon.as_str()).unwrap_or("")).size(54.0));
                        }
                    }
                    let name = match it { Some(x) => format!("{} {}", x.icon, x.name), None if kind == "hat" => "No hat".into(), None => "Midnight".into() };
                    ui.label(RichText::new(name).font(bold(15.0)));
                    if let Some(x) = it {
                        if !x.desc.is_empty() {
                            let extra = if x.id == "freeze" { format!(" You have {}/{}.", app.dash.freezes, game::MAX_FREEZES) } else { String::new() };
                            ui.label(RichText::new(format!("{}.{extra}", x.desc)).size(12.5).color(p.muted));
                        }
                    }
                    let bw = w - 40.0;
                    match it {
                        Some(x) if x.kind != "powerup" && owned.contains(&x.id) => {
                            if is_eq {
                                widgets::button_sized(ui, "✓ Equipped", p.surface2, false, 12.0, bw);
                            } else if widgets::button_sized(ui, "Equip", p.primary, true, 12.0, bw).clicked() {
                                action = Some(("equip", x.id.clone()));
                            }
                        }
                        Some(x) => {
                            let can = gems >= x.price && !(x.id == "freeze" && app.dash.freezes >= game::MAX_FREEZES);
                            if widgets::button_sized(ui, &format!("💎 {}", x.price), p.gold, can, 12.0, bw).clicked() {
                                action = Some(("buy", x.id.clone()));
                            }
                        }
                        None => {
                            if is_eq {
                                widgets::button_sized(ui, "✓ Equipped", p.surface2, false, 12.0, bw);
                            } else if widgets::button_sized(ui, "Equip", p.primary, true, 12.0, bw).clicked() {
                                action = Some(("unequip", kind.to_string()));
                            }
                        }
                    }
                });
            });
        });
    }
    match action {
        Some(("buy", id)) => match game::buy(&app.db, &id) {
            Ok(ev) => {
                app.sound.play("achievement");
                app.confetti(80);
                app.toast("🛍️", "Purchased!", "", 1);
                app.handle(&ev, false);
            }
            Err(e) => app.error(&e),
        },
        Some(("equip", id)) => {
            if let Some(it) = shop::get(&id) {
                app.db.set(&it.kind, &Some(id));
                app.sound.play("tap");
                app.refresh();
            }
        }
        Some(("unequip", kind)) => {
            app.db.set::<Option<String>>(&kind, &None);
            app.refresh();
        }
        _ => {}
    }
}

#[derive(Default)]
pub struct SettingsForm {
    pub loaded: bool,
    pub name: String,
    pub mode: String,
    pub keys: [String; 5], // gemini groq mistral openrouter claude (new values to save)
    pub gemini_model: String,
    pub claude_model: String,
    pub goal: i64,
    pub sound: bool,
    pub reminders: bool,
    pub reminder_time: String,
    pub color_emoji: bool,
}

const FREE_AIS: [(&str, &str, &str, &str); 4] = [
    ("gemini", "Gemini", "https://aistudio.google.com/apikey", "reads textbooks & photos, teaches"),
    ("groq", "Groq", "https://console.groq.com/keys", "super-fast practice questions"),
    ("mistral", "Mistral", "https://console.mistral.ai/api-keys", "grades written answers"),
    ("openrouter", "OpenRouter", "https://openrouter.ai/keys", "backup for everything"),
];

pub fn settings(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    if !app.settings.loaded {
        let db = &app.db;
        app.settings = SettingsForm {
            loaded: true,
            name: db.get::<String>("name").unwrap_or_default(),
            mode: db.mode(),
            keys: Default::default(),
            gemini_model: llm::gemini_model(db),
            claude_model: llm::claude_model(db),
            goal: db.get_or("daily_goal", 50),
            sound: db.get_or("sound", true),
            reminders: db.get_or("reminders", false),
            reminder_time: db.get_or("reminder_time", "19:00".to_string()),
            color_emoji: db.get_or("color_emoji", true),
        };
    }
    super::heading(ui, &p, "Settings", "");
    let mut save = false;
    let mut test_keys = false;
    card(&p).show(ui, |ui| {
        ui.set_width(ui.available_width().min(680.0));
        let s = &mut app.settings;
        ui.label(RichText::new("Your name").font(bold(14.0)).color(p.muted));
        ui.add(egui::TextEdit::singleline(&mut s.name).hint_text("What should Lumi call you?").desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
        ui.add_space(10.0);
        ui.label(RichText::new("AI that powers Lumi").font(bold(14.0)).color(p.muted));
        ui.horizontal(|ui| {
            for (m, title, sub) in [("free", "✨ Free · multi-AI", "Gemini + Groq + Mistral + OpenRouter"), ("claude", "🧠 Claude", "Paid · best teaching quality")] {
                let sel = s.mode == m;
                let r = egui::Frame::new().fill(if sel { widgets::lerp(p.surface, p.primary, 0.18) } else { p.surface2 }).stroke(Stroke::new(2.0, if sel { p.primary } else { p.line })).corner_radius(14).inner_margin(12).show(ui, |ui| {
                    ui.set_width(270.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(title).font(bold(15.0)));
                        ui.label(RichText::new(sub).size(12.5).color(p.muted));
                    });
                });
                if r.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    s.mode = m.into();
                }
            }
        });
        ui.add_space(8.0);
        if s.mode == "free" {
            ui.label(RichText::new("Each free AI does the jobs it's best at. If one is busy or out of free quota, the job moves to the next automatically. One key is enough to start; more keys mean more free capacity. Free tiers may use what you send to improve their AI, so avoid uploading anything private.").size(13.0).color(p.faint));
            for (i, (id, name, url, hint)) in FREE_AIS.iter().enumerate() {
                ui.add_space(6.0);
                let saved = app.db.key(id).is_some();
                let status = app.keys_tested.iter().find(|k| k.0 == *id).map(|k| match k.1 { Some(true) => "✅ working".to_string(), Some(false) => format!("❌ {}", k.2), None => "no key".into() });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{name} key")).font(bold(14.0)).color(p.muted));
                    ui.label(RichText::new(status.unwrap_or_else(|| if saved { "✅ saved".into() } else { String::new() })).size(13.0));
                });
                ui.add(egui::TextEdit::singleline(&mut s.keys[i]).password(true).hint_text(if saved { "•••••••• saved (paste to replace)" } else { "Paste your key" }).desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Free at").size(12.5).color(p.faint));
                    ui.hyperlink_to(RichText::new(url.trim_start_matches("https://")).size(12.5), *url);
                    ui.label(RichText::new(format!("· {hint}")).size(12.5).color(p.faint));
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::button_sized(ui, "🔌 Test my keys", p.surface2, true, 12.0, 0.0).clicked() {
                    test_keys = true;
                }
                ui.label(RichText::new("Gemini model").size(13.0).color(p.muted));
                egui::ComboBox::from_id_salt("gm").selected_text(llm::GEMINI_MODELS.iter().find(|m| m.0 == s.gemini_model).map(|m| m.1).unwrap_or("")).show_ui(ui, |ui| {
                    for (id, label) in llm::GEMINI_MODELS {
                        ui.selectable_value(&mut s.gemini_model, id.to_string(), *label);
                    }
                });
            });
            ui.add_space(10.0);
            egui::Frame::new().fill(p.bg).corner_radius(14).inner_margin(14).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Who does what").font(bold(15.0)));
                egui::Grid::new("routes").num_columns(2).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
                    for (_, label, chain) in llm::ROUTES {
                        ui.label(RichText::new(*label).color(p.muted));
                        ui.horizontal(|ui| {
                            for (k, prov) in chain.iter().enumerate() {
                                if k > 0 {
                                    ui.label(RichText::new("→").color(p.faint));
                                }
                                ui.label(RichText::new(llm::label(prov)).font(if k == 0 { bold(14.0) } else { egui::FontId::proportional(14.0) }).color(if k == 0 { p.primary2 } else { p.faint }));
                            }
                        });
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("First = main AI · the rest are automatic backups").size(12.0).color(p.faint));
            });
        } else {
            let saved = app.db.key("claude").is_some();
            ui.label(RichText::new(format!("Claude API key {}", if saved { "✅" } else { "" })).font(bold(14.0)).color(p.muted));
            ui.add(egui::TextEdit::singleline(&mut s.keys[4]).password(true).hint_text(if saved { "•••••••• saved (paste to replace)" } else { "sk-ant-..." }).desired_width(f32::INFINITY).margin(vec2(12.0, 10.0)));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Get one at").size(12.5).color(p.faint));
                ui.hyperlink_to(RichText::new("console.anthropic.com").size(12.5), "https://console.anthropic.com/settings/keys");
                ui.label(RichText::new("(pay-as-you-go, separate from a Claude Pro subscription)").size(12.5).color(p.faint));
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Claude model").font(bold(14.0)).color(p.muted));
            egui::ComboBox::from_id_salt("cm").selected_text(llm::CLAUDE_MODELS.iter().find(|m| m.0 == s.claude_model).map(|m| m.1).unwrap_or("")).show_ui(ui, |ui| {
                for (id, label) in llm::CLAUDE_MODELS {
                    ui.selectable_value(&mut s.claude_model, id.to_string(), *label);
                }
            });
        }
        ui.add_space(14.0);
        ui.label(RichText::new("Daily goal").font(bold(14.0)).color(p.muted));
        ui.horizontal(|ui| {
            for (g, l) in [(30, "Casual"), (50, "Regular"), (100, "Serious"), (200, "Intense")] {
                let sel = s.goal == g;
                let r = egui::Frame::new().fill(if sel { widgets::lerp(p.surface, p.primary, 0.18) } else { p.surface2 }).stroke(Stroke::new(2.0, if sel { p.primary } else { p.line })).corner_radius(12).inner_margin(10).show(ui, |ui| {
                    ui.set_width(120.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(l).font(bold(14.0)));
                        ui.label(RichText::new(format!("{g} XP/day")).size(12.0).color(p.muted));
                    });
                });
                if r.response.interact(Sense::click()).clicked() {
                    s.goal = g;
                }
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Sound effects").font(bold(14.0)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| if super::courses::toggle_switch(ui, &p, s.sound).clicked() { s.sound = !s.sound });
        });
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Color emoji").font(bold(14.0)));
                ui.label(RichText::new("Uses your system's color emoji font. Turn off to save a little memory on slow computers (takes effect after restart).").size(12.5).color(p.faint));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| if super::courses::toggle_switch(ui, &p, s.color_emoji).clicked() { s.color_emoji = !s.color_emoji });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Daily reminder").font(bold(14.0)));
                ui.label(RichText::new("One desktop notification a day, only if you haven't studied yet. Works even when Lumi is closed.").size(12.5).color(p.faint));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if super::courses::toggle_switch(ui, &p, s.reminders).clicked() {
                    s.reminders = !s.reminders;
                }
                ui.add(egui::TextEdit::singleline(&mut s.reminder_time).desired_width(60.0)).on_hover_text("24-hour time, like 19:00");
            });
        });
        ui.add_space(14.0);
        save = widgets::button(ui, "Save", p.primary, true).clicked();
        ui.add_space(10.0);
        ui.label(RichText::new(format!("Your data lives in {}", app.db.data_dir.display())).size(12.5).color(p.faint));
        ui.label(RichText::new(format!("Lumi {}", env!("CARGO_PKG_VERSION"))).size(12.5).color(p.faint));
    });
    if test_keys {
        app.keys_tested.clear();
        app.toast("🔌", "Testing your keys…", "", 1);
        app.spawn(|db, tx| {
            let r = llm::test_keys(db).into_iter().map(|(p, ok, m)| (p.to_string(), ok, m)).collect();
            let _ = tx.send(Msg::KeysTested(r));
        });
    }
    if save {
        let s = &app.settings;
        let db = &app.db;
        db.set("name", &s.name.trim().chars().take(40).collect::<String>());
        db.set("provider", &s.mode);
        for (i, id) in ["gemini", "groq", "mistral", "openrouter", "claude"].iter().enumerate() {
            let k = s.keys[i].trim();
            if !k.is_empty() {
                db.set(&if *id == "claude" { "api_key".to_string() } else { format!("{id}_key") }, &Some(k.to_string()));
            }
        }
        db.set("gemini_model", &s.gemini_model);
        db.set("model", &s.claude_model);
        db.set("daily_goal", &s.goal.clamp(10, 500));
        db.set("sound", &s.sound);
        db.set("color_emoji", &s.color_emoji);
        let t = s.reminder_time.trim();
        if game::date_of("2000-01-01").is_some() && t.len() == 5 && t.as_bytes()[2] == b':' {
            db.set("reminder_time", &t.to_string());
        }
        let was: bool = db.get_or("reminders", false);
        db.set("reminders", &s.reminders);
        if was != s.reminders || s.reminders {
            crate::reminders::configure(s.reminders);
        }
        app.settings.keys = Default::default();
        app.settings.loaded = false;
        app.toast("✅", "Saved!", "", 1);
        app.sound.enabled = app.db.get_or("sound", true);
        app.sound.play("correct");
        app.refresh();
    }
}

pub fn welcome(app: &mut App, ctx: &egui::Context) -> bool {
    let p = app.pal;
    let mut keep = true;
    modal_frame(ctx, &p, "welcome", 480.0, |ui| {
        ui.vertical_centered(|ui| {
            app.mascot(ui, 130.0, Mood::Happy);
            ui.label(RichText::new("Hi, I'm Lumi! ✨").font(bold(28.0)));
            ui.label(RichText::new("Your personal tutor. Here's how we'll work together:").color(p.muted));
        });
        ui.add_space(10.0);
        for (icon, title, sub) in [("📚", "1. Add your materials", "Textbooks, notes, quizzes, problem sets"), ("🗺️", "2. Get a learning path", "Every concept, from zero to mastery, in order"),
            ("💬", "3. Learn & practice", "1-on-1 lessons, bite-sized quizzes, spaced reviews"), ("🔥", "4. Build the habit", "Streaks, XP, crowns, quests and bosses keep you going")] {
            card(&p).inner_margin(12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon).size(26.0));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(title).font(bold(15.0)));
                        ui.label(RichText::new(sub).size(13.0).color(p.muted));
                    });
                });
            });
            ui.add_space(6.0);
        }
        ui.add_space(8.0);
        let w = ui.available_width();
        let label = if app.dash.has_key { "Let's go!" } else { "Set up my free AI keys" };
        if widgets::button_sized(ui, label, p.primary, true, 15.0, w).clicked() {
            keep = false;
            app.db.set("welcomed", &true);
            if !app.dash.has_key {
                app.go(Route::Settings);
            }
        }
    });
    keep
}
