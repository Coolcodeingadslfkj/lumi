//! Lessons with Lumi: streamed replies, tappable check questions, photos, page pull-ups.

use super::md::{self, Segment};
use super::widgets::{self, bold, Mood};
use super::{App, Msg, Pick, Route, Tab};
use crate::ai::{self, DisplayMsg};
use crate::game::Events;
use crate::llm::Ev;
use crate::model::{Concept, Course, Session};
use egui::{vec2, Align, Color32, Layout, RichText, Stroke, Ui};
use serde_json::{json, Value};

pub enum ChatEv {
    Provider(String),
    Thinking(String),
    Text(String),
    Reset,
    Done(Result<(Events, String, bool), String>),
}

pub struct Streaming {
    pub text: String,
    pub thinking: String,
    pub provider: String,
}

pub struct State {
    pub sid: i64,
    pub msgs: Vec<DisplayMsg>,
    pub input: String,
    pub streaming: Option<Streaming>,
    pub error: Option<String>,
    pub combo: i64,
    pub auto_started: bool,
}

fn load(app: &App, sid: i64) -> State {
    let msgs = Session::get(&app.db, sid).map(|s| ai::display_messages(&app.db, &s)).unwrap_or_default();
    State { sid, msgs, input: String::new(), streaming: None, error: None, combo: 0, auto_started: false }
}

pub fn send(app: &mut App, text: String, auto: bool) {
    let Some(st) = app.learn.as_mut() else { return };
    if st.streaming.is_some() {
        return;
    }
    let images = std::mem::take(&mut app.pending_images);
    let sid = st.sid;
    let shown = if text.is_empty() && !images.is_empty() { "(See the attached image.)".to_string() } else { text.clone() };
    st.msgs.push(DisplayMsg { user: true, text: shown.clone(), images: images.iter().map(|i| i.0.clone()).collect(), pages: vec![], via: None });
    st.streaming = Some(Streaming { text: String::new(), thinking: String::new(), provider: String::new() });
    st.error = None;
    let _ = auto;
    app.spawn(move |db, tx| {
        let send = |e: ChatEv| {
            let _ = tx.send(Msg::Chat(sid, e));
        };
        let r = ai::chat_turn(db, sid, &shown, &images, &mut |ev| match ev {
            Ev::Provider(p) => send(ChatEv::Provider(crate::llm::label(p).into())),
            Ev::Thinking(t) => send(ChatEv::Thinking(t)),
            Ev::Text(t) => send(ChatEv::Text(t)),
            Ev::Reset => send(ChatEv::Reset),
        });
        send(ChatEv::Done(r.map(|d| (d.events, crate::llm::label(d.via).to_string(), d.truncated)).map_err(|e| e.friendly(db.mode() == "free"))));
    });
}

pub fn on_event(app: &mut App, sid: i64, ev: ChatEv) {
    let Some(st) = app.learn.as_mut().filter(|s| s.sid == sid) else {
        if let ChatEv::Done(Ok((events, _, _))) = ev {
            app.handle(&events, false); // finished after the learner left the lesson
        }
        return;
    };
    match ev {
        ChatEv::Provider(p) => {
            if let Some(s) = &mut st.streaming {
                s.provider = p;
            }
        }
        ChatEv::Thinking(t) => {
            if let Some(s) = &mut st.streaming {
                s.thinking.push_str(&t);
            }
        }
        ChatEv::Text(t) => {
            if let Some(s) = &mut st.streaming {
                s.text.push_str(&t);
            }
        }
        ChatEv::Reset => {
            if let Some(s) = &mut st.streaming {
                s.text.clear();
                s.thinking.clear();
            }
        }
        ChatEv::Done(r) => {
            st.streaming = None;
            match r {
                Ok((events, _, truncated)) => {
                    let fresh = load(app, sid);
                    let st = app.learn.as_mut().unwrap();
                    st.msgs = fresh.msgs;
                    if truncated {
                        st.error = Some("(The reply was cut off. Ask Lumi to continue.)".into());
                    }
                    app.handle(&events, false);
                }
                Err(e) => {
                    st.error = Some(e.clone());
                    app.sound.play("wrong");
                }
            }
        }
    }
}

pub fn add_images(app: &mut App, files: Vec<std::path::PathBuf>) {
    for f in files.into_iter().take(5) {
        app.spawn(move |db, tx| {
            let r = (|| -> Result<(String, String), String> {
                let img = image::open(&f).map_err(|_| format!("Couldn't read {}", f.display()))?;
                // downscale to the size AIs actually use, and re-encode small
                let img = if img.width().max(img.height()) > 1568 { img.resize(1568, 1568, image::imageops::FilterType::Triangle) } else { img };
                let name = format!("{}.jpg", crate::materials::random_hex());
                let rgb = img.to_rgb8();
                let mut out = std::io::Cursor::new(vec![]);
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88).encode_image(&rgb).map_err(|e| e.to_string())?;
                std::fs::write(db.path("chat_images").join(&name), out.into_inner()).map_err(|e| e.to_string())?;
                Ok((name, "image/jpeg".into()))
            })();
            let _ = tx.send(Msg::ChatImage(r));
        });
    }
}

fn thumb(app: &mut App, name: &str) -> Option<egui::TextureHandle> {
    if let Some(t) = app.chat_textures.get(name) {
        return Some(t.clone());
    }
    let path = app.db.path("chat_images").join(std::path::Path::new(name).file_name()?);
    let img = image::open(path).ok()?.thumbnail(480, 480).to_rgba8();
    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], &img);
    let t = app.ctx.load_texture(name, ci, egui::TextureOptions::LINEAR);
    if app.chat_textures.len() > 40 {
        app.chat_textures.clear(); // keep memory bounded
    }
    app.chat_textures.insert(name.to_string(), t.clone());
    Some(t)
}

pub fn show(app: &mut App, ui: &mut Ui, sid: i64) {
    if app.learn.as_ref().map(|s| s.sid != sid).unwrap_or(true) {
        app.learn = Some(load(app, sid));
    }
    let Some(session) = Session::get(&app.db, sid) else {
        app.go(Route::Courses);
        return;
    };
    let p = app.pal;
    let course = Course::get(&app.db, session.course_id).unwrap_or_default();
    let concept = session.concept_id.and_then(|c| Concept::get(&app.db, c));
    // auto-start a concept lesson
    let needs_start = { let st = app.learn.as_ref().unwrap(); st.msgs.is_empty() && !st.auto_started && st.streaming.is_none() };
    if needs_start {
        if let Some(c) = &concept {
            app.learn.as_mut().unwrap().auto_started = true;
            let t = if session.test_id.is_some() { format!("I'm prepping for my test. Teach me \"{}\", focused on what the test will ask.", c.title) } else { format!("Let's start! Teach me \"{}\".", c.title) };
            send(app, t, true);
        }
    }
    egui::Panel::top("learn_head").frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 4, bottom: 10 })).show(ui, |ui| {
        ui.horizontal(|ui| {
            if widgets::button_sized(ui, "←", p.surface2, true, 13.0, 0.0).clicked() {
                match session.test_id {
                    Some(t) => app.go(Route::Test(t)),
                    None => app.go(Route::Course(session.course_id, Tab::Lessons)),
                }
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(&session.title).font(bold(18.0)));
                let info = match &concept {
                    Some(c) => format!("{} {} · {} · {}% mastery", course.emoji, course.name, if c.level() > 0 { "👑".repeat(c.level() as usize) } else { "new concept".into() }, c.mastery as i64),
                    None => format!("{} {}", course.emoji, course.name),
                };
                ui.label(RichText::new(info).size(13.0).color(p.muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(c) = &concept {
                    let (label, kind) = if session.test_id.is_some() { ("✅ Ready to practice", Some("practice".to_string())) } else { ("⚡ Practice this", None) };
                    if widgets::button_sized(ui, label, p.green, true, 13.0, 0.0).clicked() {
                        super::quiz::start(app, super::quiz::Request {
                            course_id: Some(session.course_id), concept_ids: vec![c.id], count: 8, test_id: session.test_id, kind,
                            return_to: session.test_id.map(Route::Test), ..Default::default()
                        });
                    }
                }
            });
        });
        ui.separator();
    });
    egui::Panel::bottom("learn_compose").frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 8, bottom: 14 })).show(ui, |ui| composer(app, ui, &session, concept.is_some()));
    egui::CentralPanel::no_frame().show(ui, |ui| {
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            super::column(ui, 860.0, |ui| messages(app, ui, &session, &course));
        });
    });
}

fn messages(app: &mut App, ui: &mut Ui, session: &Session, course: &Course) {
    let p = app.pal;
    let st_msgs = app.learn.as_ref().unwrap().msgs.clone();
    if st_msgs.is_empty() && session.concept_id.is_none() && app.learn.as_ref().unwrap().streaming.is_none() {
        assistant_bubble(app, ui, |app, ui| {
            let name = if app.dash.name.is_empty() { String::new() } else { format!(" {}", app.dash.name) };
            md::show(ui, &mut app.md_cache, &format!("Hey{name}! 👋 I've got your **{}** materials open. What are we working on? You can paste a problem, name a chapter or page, send a photo, or ask me to quiz you.", course.name));
        });
    }
    for (i, m) in st_msgs.iter().enumerate() {
        if m.user {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                egui::Frame::new().fill(p.primary).corner_radius(egui::CornerRadius { nw: 20, ne: 4, sw: 20, se: 20 }).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
                    ui.set_max_width(560.0);
                    ui.vertical(|ui| {
                        if !m.images.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                for name in &m.images {
                                    if let Some(t) = thumb(app, name) {
                                        let s = t.size_vec2();
                                        let k = (220.0 / s.x).min(220.0 / s.y).min(1.0);
                                        if ui.add(egui::Image::new((t.id(), s * k)).corner_radius(10).sense(egui::Sense::click())).clicked() {
                                            app.dialog = Some(super::Dialog::Image(name.clone()));
                                        }
                                    }
                                }
                            });
                        }
                        let shown = if m.text == "(See the attached image.)" { String::new() } else if let Some(r) = m.text.strip_prefix("[Check question result] ") { format!("🧩 {r}") } else { m.text.clone() };
                        if !shown.is_empty() {
                            ui.label(RichText::new(shown).color(Color32::WHITE));
                        }
                        if !m.pages.is_empty() {
                            ui.label(RichText::new(format!("📖 Pulled up {}", m.pages.join("; "))).size(11.0).color(Color32::from_white_alpha(190)));
                        }
                    });
                });
            });
        } else {
            let text = m.text.clone();
            let via = m.via.clone();
            assistant_bubble(app, ui, |app, ui| {
                render_reply(app, ui, &text, i, false);
                if let Some(v) = via {
                    ui.label(RichText::new(format!("via {v}")).size(11.0).color(app.pal.faint));
                }
            });
        }
        ui.add_space(12.0);
    }
    let streaming = app.learn.as_ref().unwrap().streaming.as_ref().map(|s| (s.text.clone(), s.thinking.clone(), s.provider.clone()));
    if let Some((text, thinking, provider)) = streaming {
        let idx = st_msgs.len();
        assistant_bubble(app, ui, |app, ui| {
            if text.is_empty() {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().color(app.pal.primary2));
                    ui.label(RichText::new(if provider.is_empty() { "Lumi is thinking…".to_string() } else { format!("{provider} is thinking…") }).color(app.pal.faint));
                });
                if !thinking.is_empty() {
                    egui::CollapsingHeader::new(RichText::new("Thought process").size(12.0).color(app.pal.faint)).show(ui, |ui| {
                        ui.label(RichText::new(&thinking).size(12.0).color(app.pal.faint));
                    });
                }
            } else {
                render_reply(app, ui, &text, idx, true);
            }
        });
        ui.ctx().request_repaint();
    }
    if let Some(e) = app.learn.as_ref().unwrap().error.clone() {
        ui.label(RichText::new(format!("😵 {e}")).color(p.red));
    }
}

fn assistant_bubble(app: &mut App, ui: &mut Ui, add: impl FnOnce(&mut App, &mut Ui)) {
    let p = app.pal;
    ui.horizontal_top(|ui| {
        app.mascot(ui, 42.0, Mood::Idle);
        egui::Frame::new().fill(p.surface).stroke(Stroke::new(2.0, p.line)).corner_radius(egui::CornerRadius { nw: 4, ne: 20, sw: 20, se: 20 }).inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(720.0));
            ui.vertical(|ui| add(app, ui));
        });
    });
}

fn render_reply(app: &mut App, ui: &mut Ui, text: &str, msg_idx: usize, live: bool) {
    for (ci, seg) in md::segments(text, live).into_iter().enumerate() {
        match seg {
            Segment::Md(t) if !t.trim().is_empty() => md::show(ui, &mut app.md_cache, &t),
            Segment::Md(_) => {}
            Segment::Pending => {
                ui.label(RichText::new("✏️ Preparing a question for you…").italics().color(app.pal.muted));
            }
            Segment::Check(q) => check_card(app, ui, &q, msg_idx, ci, live),
        }
    }
}

fn check_card(app: &mut App, ui: &mut Ui, q: &Value, msg_idx: usize, ci: usize, live: bool) {
    let p = app.pal;
    let sid = app.learn.as_ref().unwrap().sid;
    let key = format!("check_{sid}_{msg_idx}_{ci}");
    let saved: Option<i64> = app.db.get(&key);
    let opts: Vec<String> = q["options"].as_array().into_iter().flatten().filter_map(|o| o.as_str().map(String::from)).collect();
    let answer = q["answer"].as_i64().unwrap_or(0);
    let mut picked: Option<i64> = None;
    egui::Frame::new().fill(p.surface2).stroke(Stroke::new(2.0, p.line)).corner_radius(16).inner_margin(14).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(format!("🧩 {}", md::math_to_unicode(q["question"].as_str().unwrap_or("")))).font(bold(15.0)));
        for (i, o) in opts.iter().enumerate() {
            let i = i as i64;
            let (fill, stroke) = match saved {
                Some(_) if i == answer => (widgets::lerp(p.surface, p.green, 0.2), p.green),
                Some(s) if i == s => (widgets::lerp(p.surface, p.red, 0.2), p.red),
                _ => (p.surface, p.line),
            };
            let r = egui::Frame::new().fill(fill).stroke(Stroke::new(2.0, stroke)).corner_radius(12).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(md::math_to_unicode(o));
            });
            if saved.is_none() && !live && r.response.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                picked = Some(i);
            }
        }
        if let Some(s) = saved {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{} {}", if s == answer { "✅" } else { "❌" }, md::math_to_unicode(q["explanation"].as_str().unwrap_or("")))).color(p.muted));
        }
    });
    if let Some(choice) = picked {
        app.db.set(&key, &choice);
        let ok = choice == answer;
        let st = app.learn.as_mut().unwrap();
        st.combo = if ok { st.combo + 1 } else { 0 };
        let combo = st.combo;
        app.sound.play(if ok { "correct" } else { "wrong" });
        let session = Session::get(&app.db, sid);
        let concept_id = session.as_ref().and_then(|s| s.concept_id);
        let question = json!({"type": "multiple_choice", "prompt": q["question"], "options": opts, "correct_index": answer, "explanation": q["explanation"],
            "accepted_answers": [], "pairs": [], "ordered_items": [], "model_answer": "", "rubric": "", "hint": "", "concept_id": concept_id.unwrap_or(0)});
        let ua = opts.get(choice as usize).cloned().unwrap_or_default();
        let ev = crate::actions::answer(&app.db, &crate::actions::Answer {
            concept_id, course_id: session.as_ref().map(|s| s.course_id), correct: ok, combo, retry: false,
            question: Some(&question), user_answer: &ua, mistake_id: None, from_check_card: true,
        });
        let at = ui.ctx().pointer_latest_pos().unwrap_or(ui.min_rect().center());
        app.float_xp(ev.xp, at);
        app.handle(&ev, false);
        let msg = format!("[Check question result] I chose \"{ua}\". That was {}.", if ok { "correct ✅".to_string() } else { format!("wrong ❌ (the answer was \"{}\")", opts.get(answer as usize).cloned().unwrap_or_default()) });
        send(app, msg, true);
    }
}

fn composer(app: &mut App, ui: &mut Ui, session: &Session, has_concept: bool) {
    let p = app.pal;
    super::column(ui, 860.0, |ui| {
        let quick: &[&str] = if session.test_id.is_some() {
            &["What will the test ask about this?", "Show me a past-exam style problem", "What are the common traps?", "Explain it more simply", "Quiz me like the test"]
        } else if has_concept {
            &["Explain it more simply", "Show me a worked example", "Quiz me on this", "Why does this matter?", "Give me a practice problem", "I've got it, what's next?"]
        } else {
            &["Help me with a problem from my materials", "What should I study first?", "Summarize the key ideas", "Make me a study plan", "Quiz me on everything"]
        };
        let busy = app.learn.as_ref().map(|s| s.streaming.is_some()).unwrap_or(true);
        let mut chosen = None;
        // flow the chips into rows that fit the width
        let width = ui.available_width();
        let mut rows: Vec<Vec<&str>> = vec![vec![]];
        let mut x = 0.0;
        for q in quick {
            let w = ui.painter().layout_no_wrap(q.to_string(), bold(13.0), p.muted).size().x + 36.0;
            if x + w > width && !rows.last().unwrap().is_empty() {
                rows.push(vec![]);
                x = 0.0;
            }
            rows.last_mut().unwrap().push(q);
            x += w;
        }
        for row in rows {
            ui.horizontal(|ui| {
                for q in row {
                    let r = egui::Frame::new().fill(p.surface).stroke(Stroke::new(2.0, p.line)).corner_radius(99).inner_margin(egui::Margin::symmetric(12, 6))
                        .show(ui, |ui| ui.add(egui::Label::new(RichText::new(q).font(bold(13.0)).color(p.muted)).extend()));
                    if !busy && r.response.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        chosen = Some(q.to_string());
                    }
                }
            });
        }
        if let Some(q) = chosen {
            send(app, q, false);
        }
        if !app.pending_images.is_empty() {
            let imgs = app.pending_images.clone();
            let mut remove = None;
            ui.horizontal(|ui| {
                for (i, (name, _)) in imgs.iter().enumerate() {
                    if let Some(t) = thumb(app, name) {
                        let s = t.size_vec2();
                        let k = (72.0 / s.x).min(72.0 / s.y);
                        ui.add(egui::Image::new((t.id(), s * k)).corner_radius(10));
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                    }
                }
            });
            if let Some(i) = remove {
                app.pending_images.remove(i);
            }
        }
        ui.add_space(4.0);
        let mut submit = false;
        ui.horizontal(|ui| {
            if widgets::button_sized(ui, "📷", p.surface2, true, 15.0, 0.0).on_hover_text("Add a photo: homework, a textbook page, your handwritten work (or drag images in)").clicked() {
                app.pick_files(Pick::ChatImages, true);
            }
            let st = app.learn.as_mut().unwrap();
            let w = ui.available_width() - 130.0;
            let r = ui.add(egui::TextEdit::multiline(&mut st.input).hint_text("Ask anything, answer Lumi's question, or paste a problem…").desired_rows(1).desired_width(w).margin(vec2(12.0, 10.0)));
            if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
                submit = true;
            }
            let can = !busy && (!st.input.trim().is_empty() || !app.pending_images.is_empty());
            if widgets::button(ui, "Send", p.primary, can).clicked() {
                submit = true;
            }
        });
        if submit && !busy {
            let st = app.learn.as_mut().unwrap();
            let text = st.input.trim().to_string();
            if !text.is_empty() || !app.pending_images.is_empty() {
                st.input.clear();
                send(app, text, false);
            }
        }
    });
}
