//! The desktop UI (egui): app state, navigation, background work, celebrations.

pub mod courses;
pub mod home;
pub mod learn;
pub mod md;
pub mod misc;
pub mod quiz;
pub mod tests;
pub mod theme;
pub mod widgets;

use crate::db::Shared;
use crate::game::{self, Events, Stage};
use egui::{vec2, Align, Color32, Layout, RichText, Ui};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender};
use theme::Pal;
use widgets::{bold, Fx, Mood};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Path,
    Materials,
    Lessons,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum Route {
    #[default]
    Home,
    Courses,
    Course(i64, Tab),
    Learn(i64),
    Review,
    Mistakes(Option<i64>),
    Achievements,
    Shop,
    Tests,
    Test(i64),
    Settings,
}

/// Results coming back from background threads.
pub enum Msg {
    Curriculum(i64, Result<Value, String>),
    Analyzed(i64, Result<(), String>),
    Quiz(Box<quiz::Request>, Result<quiz::Data, String>),
    Chat(i64, learn::ChatEv),
    Explain(u64, Option<String>, bool), // text chunk (None = reset), done
    Graded(u64, Result<(Value, String), String>),
    Uploaded(i64, usize, Vec<String>),
    Picked(Pick, Vec<std::path::PathBuf>),
    ChatImage(Result<(String, String), String>),
    KeysTested(Vec<(String, Option<bool>, String)>),
    Toast(String, String, String),
}

#[derive(Clone, Debug)]
pub enum Pick {
    Materials(i64),
    ChatImages,
    TestFiles,
}

#[derive(Default, Clone)]
pub struct Dash {
    pub xp: i64,
    pub level: (i64, i64, i64),
    pub streak: i64,
    pub today_xp: i64,
    pub today_answers: i64,
    pub today_correct: i64,
    pub goal: i64,
    pub freezes: i64,
    pub gems: i64,
    pub hat: Option<String>,
    pub theme: Option<String>,
    pub stage_key: String,
    pub stage: Option<Stage>,
    pub due: usize,
    pub mistakes: i64,
    pub achievements: i64,
    pub name: String,
    pub has_key: bool,
    pub mode: String,
}

pub struct Toast {
    pub icon: String,
    pub title: String,
    pub sub: String,
    pub kind: u8, // 0 achievement, 1 info, 2 error
    pub born: f64,
}

pub enum Reward {
    Goal,
    LevelUp(i64),
    Evolved(Stage),
}

pub struct Busy {
    pub title: String,
    pub tips: Vec<&'static str>,
    pub started: f64,
    pub hidden: bool,
}

pub const TIPS: &[&str] = &[
    "Tip: testing yourself beats re-reading. That's why Lumi quizzes you so often.",
    "Tip: spacing practice over days makes memories last far longer than cramming.",
    "Tip: mistakes are great data. Missed questions come back until you nail them.",
    "Tip: explaining an idea in your own words is one of the fastest ways to learn it.",
    "Tip: mixing topics during review (interleaving) builds stronger understanding.",
    "Tip: a 10-minute session every day beats a 2-hour session once a week.",
    "Reading your materials carefully…",
    "Finding the tricky parts…",
];

pub enum Dialog {
    NewCourse { name: String, goal: String, emoji: String },
    Note { course: i64, title: String, text: String },
    Confirm { title: String, body: String, action: Confirm },
    NewTest(Box<tests::NewTest>),
    EditTest { id: i64, name: String, date: String, topics: String },
    ConceptMenu { course: i64, concept: i64 },
    BossIntro { course: i64, unit: usize },
    Chest { gems: i64, freeze: bool, opened_at: f64 },
    Image(String),
    Welcome,
}

pub enum Confirm {
    DeleteCourse(i64),
    DeleteTest(i64),
    QuitQuiz,
}

pub struct App {
    pub db: Shared,
    pub route: Route,
    pub pal: Pal,
    pub dash: Dash,
    pub tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub ctx: egui::Context,
    pub toasts: Vec<Toast>,
    pub rewards: VecDeque<Reward>,
    pub dialog: Option<Dialog>,
    pub busy: Option<Busy>,
    pub quiz: Option<quiz::Run>,
    pub learn: Option<learn::State>,
    pub fx: Fx,
    pub md_cache: egui_commonmark::CommonMarkCache,
    pub settings: misc::SettingsForm,
    pub chat_textures: std::collections::HashMap<String, egui::TextureHandle>,
    pub pending_images: Vec<(String, String)>,
    pub new_test_files: Vec<std::path::PathBuf>,
    pub keys_tested: Vec<(String, Option<bool>, String)>,
    pub theme_applied: Option<Option<String>>,
    pub sound: crate::sound::Sound,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, db: Shared) -> App {
        Self::with_ctx(cc.egui_ctx.clone(), db)
    }

    pub fn with_ctx(ctx: egui::Context, db: Shared) -> App {
        let color_emoji: bool = db.get_or("color_emoji", true);
        ctx.set_fonts(theme::fonts(color_emoji));
        let (tx, rx) = channel();
        crate::materials::ensure_all_background(db.clone()); // materials added by older versions
        // people upgrading from an older Lumi already have courses and don't need the intro
        let first_run = !db.get_or("welcomed", false) && db.int("SELECT COUNT(*) FROM courses", &[]) == 0;
        let mut app = App {
            route: Route::Home,
            pal: theme::palette(None),
            dash: Dash::default(),
            tx,
            rx,
            ctx: ctx.clone(),
            toasts: vec![],
            rewards: VecDeque::new(),
            dialog: if first_run { Some(Dialog::Welcome) } else { None },
            busy: None,
            quiz: None,
            learn: None,
            fx: Fx::default(),
            md_cache: Default::default(),
            settings: misc::SettingsForm::default(),
            chat_textures: Default::default(),
            pending_images: vec![],
            new_test_files: vec![],
            keys_tested: vec![],
            theme_applied: None,
            sound: crate::sound::Sound::new(db.get_or("sound", true)),
            db,
        };
        app.refresh();
        if let Some(n) = app.db.get::<i64>("freeze_used_notice").filter(|n| *n > 0) {
            app.db.set("freeze_used_notice", &0);
            app.toast("🧊", "A streak freeze saved your streak!", &format!("Covered {n} missed day(s)."), 1);
        }
        app
    }

    /// Recompute the numbers shown in the top bar and home screen.
    pub fn refresh(&mut self) {
        let db = &self.db;
        game::apply_streak_freezes(db);
        let xp = game::total_xp(db);
        let level = game::level_for_xp(xp);
        let stage = game::stage_for_level(level.0);
        let (ta, tc) = db.one("SELECT answers, correct FROM activity WHERE day=?", &[&game::today()], |r| Ok((r.get::<_, Option<i64>>(0)?.unwrap_or(0), r.get::<_, Option<i64>>(1)?.unwrap_or(0)))).unwrap_or((0, 0));
        self.dash = Dash {
            xp,
            level,
            streak: game::streak(db),
            today_xp: game::today_xp(db),
            today_answers: ta,
            today_correct: tc,
            goal: db.get_or("daily_goal", 50),
            freezes: db.get_or("freezes", 0),
            gems: game::gems(db),
            hat: db.get::<Option<String>>("hat").flatten(),
            theme: db.get::<Option<String>>("theme").flatten(),
            stage_key: stage.key.to_string(),
            stage: Some(stage),
            due: game::due_concepts(db, None).len(),
            mistakes: db.int("SELECT COUNT(*) FROM mistakes WHERE fixed=0", &[]),
            achievements: db.int("SELECT COUNT(*) FROM achievements", &[]),
            name: db.get::<String>("name").unwrap_or_default(),
            has_key: db.has_key(),
            mode: db.mode(),
        };
        self.sound.enabled = db.get_or("sound", true);
    }

    pub fn go(&mut self, r: Route) {
        if !matches!(r, Route::Learn(_)) {
            self.learn = None;
        }
        self.route = r;
        self.refresh();
    }

    pub fn toast(&mut self, icon: &str, title: &str, sub: &str, kind: u8) {
        let born = self.ctx.input(|i| i.time);
        self.toasts.push(Toast { icon: icon.into(), title: title.into(), sub: sub.into(), kind, born });
    }

    pub fn error(&mut self, msg: &str) {
        self.toast("😵", "Oops", msg, 2);
        self.sound.play("wrong");
    }

    /// Run work on a background thread; results arrive through `Msg`.
    pub fn spawn(&self, f: impl FnOnce(&crate::db::Db, &Sender<Msg>) + Send + 'static) {
        let (db, tx, ctx) = (self.db.clone(), self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            f(&db, &tx);
            ctx.request_repaint();
        });
    }

    pub fn busy(&mut self, title: &str) {
        let started = self.ctx.input(|i| i.time);
        self.busy = Some(Busy { title: title.into(), tips: TIPS.to_vec(), started, hidden: false });
    }

    /// Celebrate what an action produced. `quiet` defers big pop-ups (used mid-quiz).
    pub fn handle(&mut self, ev: &Events, quiet: bool) {
        for k in &ev.achievements {
            if let Some(a) = game::achievement(k) {
                self.toast(a.icon, &format!("Achievement unlocked: {}", a.name), a.desc, 0);
                self.sound.play("achievement");
            }
        }
        if ev.freeze_earned {
            self.toast("🧊", "Streak freeze earned!", "It protects your streak if you miss a day.", 1);
        }
        for q in &ev.quests_completed {
            self.toast("📜", "Quest complete!", q, 1);
        }
        if ev.mistake_fixed {
            self.toast("📓", "Mistake fixed!", "Removed from your notebook.", 1);
        }
        if ev.gems > 0 && ev.chest_gems == 0 {
            let at = self.ctx.content_rect().right_top() + vec2(-260.0, 60.0);
            let now = self.ctx.input(|i| i.time);
            self.fx.float(format!("+{} 💎", ev.gems), at, now, self.pal.ice);
        }
        if !quiet {
            if ev.goal_hit {
                self.rewards.push_back(Reward::Goal);
            }
            if let Some(l) = ev.level_up {
                self.rewards.push_back(Reward::LevelUp(l));
            }
            if let Some(s) = &ev.evolved {
                self.rewards.push_back(Reward::Evolved(s.clone()));
            }
        }
        self.refresh();
    }

    pub fn float_xp(&mut self, xp: i64, at: egui::Pos2) {
        if xp > 0 {
            let now = self.ctx.input(|i| i.time);
            self.fx.float(format!("+{xp} XP"), at, now, self.pal.gold);
        }
    }

    pub fn confetti(&mut self, n: usize) {
        let r = self.ctx.content_rect();
        self.fx.confetti(r, n);
    }

    fn poll(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Curriculum(cid, r) => courses::on_curriculum(self, cid, r),
                Msg::Analyzed(tid, r) => tests::on_analyzed(self, tid, r),
                Msg::Quiz(req, r) => quiz::on_ready(self, *req, r),
                Msg::Chat(sid, ev) => learn::on_event(self, sid, ev),
                Msg::Explain(id, chunk, done) => quiz::on_explain(self, id, chunk, done),
                Msg::Graded(id, r) => quiz::on_graded(self, id, r),
                Msg::Uploaded(cid, n, errs) => courses::on_uploaded(self, cid, n, errs),
                Msg::Picked(p, files) => self.on_picked(p, files),
                Msg::ChatImage(r) => match r {
                    Ok(img) => self.pending_images.push(img),
                    Err(e) => self.error(&e),
                },
                Msg::KeysTested(v) => self.keys_tested = v,
                Msg::Toast(i, t, s) => self.toast(&i, &t, &s, 1),
            }
        }
    }

    fn on_picked(&mut self, p: Pick, files: Vec<std::path::PathBuf>) {
        if files.is_empty() {
            return;
        }
        match p {
            Pick::Materials(cid) => courses::upload(self, cid, files),
            Pick::ChatImages => learn::add_images(self, files),
            Pick::TestFiles => self.new_test_files.extend(files),
        }
    }

    pub fn pick_files(&self, p: Pick, images_only: bool) {
        self.spawn(move |_, tx| {
            let mut d = rfd::FileDialog::new();
            d = if images_only {
                d.add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
            } else {
                d.add_filter("Study materials", &["pdf", "docx", "txt", "md", "csv", "tex", "png", "jpg", "jpeg", "webp", "gif"]).add_filter("All files", &["*"])
            };
            let files = d.pick_files().unwrap_or_default();
            let _ = tx.send(Msg::Picked(p, files));
        });
    }

    fn apply_theme(&mut self) {
        if self.theme_applied.as_ref() != Some(&self.dash.theme) {
            self.pal = theme::palette(self.dash.theme.as_deref());
            theme::apply_visuals(&self.ctx, &self.pal);
            self.theme_applied = Some(self.dash.theme.clone());
        }
    }

    pub fn mascot(&self, ui: &mut Ui, size: f32, mood: Mood) {
        widgets::mascot_widget(ui, size, mood, &self.dash.stage_key, self.dash.hat.as_deref());
    }

    // ---------- frame ----------

    pub fn frame(&mut self, ui: &mut Ui) {
        self.poll();
        // lets automated screenshot tests switch screens
        if let Some(r) = ui.ctx().data_mut(|d| d.remove_temp::<Route>(egui::Id::unique("test_route"))) {
            self.go(r);
        }
        if let Some(Some((req, data))) = ui.ctx().data_mut(|d| d.remove_temp::<Option<(quiz::Request, quiz::Data)>>(egui::Id::unique("test_quiz"))) {
            quiz::on_ready(self, req, Ok(data));
        }
        self.apply_theme();
        let ctx = ui.ctx().clone();
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() {
            match &self.route {
                Route::Learn(_) => learn::add_images(self, dropped),
                Route::Course(cid, _) => {
                    let cid = *cid;
                    courses::upload(self, cid, dropped)
                }
                Route::Tests if matches!(self.dialog, Some(Dialog::NewTest(_))) => self.new_test_files.extend(dropped),
                _ => self.toast("📎", "Open a course first", "Drop files on a course's Materials tab.", 1),
            }
        }
        if self.quiz.is_some() {
            egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(self.pal.bg)).show(ui, |ui| quiz::show(self, ui));
        } else {
            egui::Panel::left("nav").exact_size(224.0).resizable(false)
                .frame(egui::Frame::new().fill(widgets::darken(self.pal.bg, 0.85)).inner_margin(egui::Margin::symmetric(14, 18)))
                .show(ui, |ui| self.sidebar(ui));
            egui::Panel::top("top").frame(egui::Frame::new().fill(self.pal.bg).inner_margin(egui::Margin { left: 28, right: 28, top: 14, bottom: 6 }))
                .show(ui, |ui| self.topbar(ui));
            egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(self.pal.bg).inner_margin(egui::Margin { left: 28, right: 28, top: 4, bottom: 0 }))
                .show(ui, |ui| {
                    if let Route::Learn(sid) = self.route {
                        learn::show(self, ui, sid);
                    } else {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            column(ui, 1080.0, |ui| {
                                self.banner(ui);
                                match self.route.clone() {
                                    Route::Home => home::show(self, ui),
                                    Route::Courses => courses::list(self, ui),
                                    Route::Course(id, tab) => courses::course(self, ui, id, tab),
                                    Route::Review => misc::review(self, ui),
                                    Route::Mistakes(c) => misc::mistakes(self, ui, c),
                                    Route::Achievements => misc::achievements(self, ui),
                                    Route::Shop => misc::shop(self, ui),
                                    Route::Tests => tests::list(self, ui),
                                    Route::Test(id) => tests::test(self, ui, id),
                                    Route::Settings => misc::settings(self, ui),
                                    Route::Learn(_) => {}
                                }
                                ui.add_space(50.0);
                            });
                        });
                    }
                });
        }
        self.dialogs(&ctx);
        self.rewards_ui(&ctx);
        self.busy_ui(&ctx);
        self.toasts_ui(&ctx);
        self.fx.paint(&ctx);
    }

    fn banner(&mut self, ui: &mut Ui) {
        let hour = chrono::Timelike::hour(&chrono::Local::now());
        if self.dash.today_xp == 0 && self.dash.streak > 0 && hour >= 17 {
            egui::Frame::new().fill(widgets::lerp(self.pal.bg, self.pal.flame, 0.15)).stroke(egui::Stroke::new(2.0, widgets::lerp(self.pal.bg, self.pal.flame, 0.5)))
                .corner_radius(16).inner_margin(12).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("🔥 Your {}-day streak is on the line! One quick practice keeps it alive.", self.dash.streak)).font(bold(15.0)).color(Color32::from_rgb(255, 210, 179)));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::button_sized(ui, "Save it", self.pal.primary, true, 12.0, 0.0).clicked() {
                                self.go(Route::Review);
                            }
                        });
                    });
                });
            ui.add_space(12.0);
        }
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            self.mascot(ui, 46.0, if self.dash.today_xp > 0 { Mood::Happy } else { Mood::Idle });
            ui.label(RichText::new("Lumi").font(bold(30.0)).color(self.pal.text));
        });
        ui.add_space(14.0);
        let items: [(&str, &str, Route); 9] = [
            ("🏠", "Home", Route::Home),
            ("📚", "Courses", Route::Courses),
            ("🎯", "Test Prep", Route::Tests),
            ("🧠", "Review", Route::Review),
            ("📓", "Mistakes", Route::Mistakes(None)),
            ("🏆", "Achievements", Route::Achievements),
            ("💎", "Shop", Route::Shop),
            ("⚙️", "Settings", Route::Settings),
            ("", "", Route::Home),
        ];
        for (icon, label, r) in items.into_iter().take(8) {
            let active = match (&self.route, &r) {
                (Route::Course(..), Route::Courses) | (Route::Learn(_), Route::Courses) | (Route::Test(_), Route::Tests) => true,
                (Route::Mistakes(_), Route::Mistakes(_)) => true,
                (a, b) => a == b,
            };
            let badge = match label {
                "Review" if self.dash.due > 0 => Some(self.dash.due as i64),
                "Mistakes" if self.dash.mistakes > 0 => Some(self.dash.mistakes),
                _ => None,
            };
            if nav_item(ui, &self.pal, icon, label, active, badge).clicked() {
                self.go(r);
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
            widgets::card(&self.pal).show(ui, |ui| {
                ui.set_width(170.0);
                ui.vertical_centered(|ui| {
                    let f = self.dash.today_xp as f32 / self.dash.goal.max(1) as f32;
                    let done = f >= 1.0;
                    widgets::ring(ui, &self.pal, 96.0, f, &if done { "✓".into() } else { self.dash.today_xp.to_string() },
                        &if done { "goal hit".into() } else { format!("/ {} XP", self.dash.goal) }, if done { self.pal.gold } else { self.pal.green });
                    ui.label(RichText::new("Daily goal").font(bold(13.0)).color(self.pal.muted));
                });
            });
        });
    }

    fn topbar(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let d = &self.dash;
            let p = self.pal;
            widgets::chip(ui, &p, &format!("⚡ {}  Lv {}", d.xp, d.level.0), p.gold).on_hover_text(format!("{}/{} XP to level {}", d.level.1, d.level.2, d.level.0 + 1));
            if widgets::chip(ui, &p, &format!("💎 {}", d.gems), p.ice).on_hover_text("Gems: spend them in the shop").interact(egui::Sense::click()).clicked() {
                self.go(Route::Shop);
                return;
            }
            let d = &self.dash;
            widgets::chip(ui, &p, &format!("🔥 {}", d.streak), if d.today_xp > 0 { p.flame } else { p.faint })
                .on_hover_text(if d.today_xp > 0 { "Streak extended today!" } else { "Study today to extend your streak" });
            widgets::chip(ui, &p, &format!("🧊 {}", d.freezes), p.ice).on_hover_text("Streak freezes protect your streak on a missed day");
        });
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.toasts.retain(|t| now - t.born < if t.kind == 2 { 7.0 } else { 4.5 });
        if self.toasts.is_empty() {
            return;
        }
        let p = self.pal;
        egui::Area::new(egui::Id::unique("toasts")).anchor(egui::Align2::RIGHT_TOP, vec2(-18.0, 18.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            for t in self.toasts.iter().rev().take(5) {
                let border = match t.kind { 0 => p.gold, 2 => p.red, _ => p.primary };
                egui::Frame::new().fill(p.surface).stroke(egui::Stroke::new(2.0, border)).corner_radius(16).inner_margin(12).show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&t.icon).size(28.0));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&t.title).font(bold(14.5)));
                            if !t.sub.is_empty() {
                                ui.label(RichText::new(&t.sub).size(13.0).color(p.muted));
                            }
                        });
                    });
                });
                ui.add_space(8.0);
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(400));
    }

    fn busy_ui(&mut self, ctx: &egui::Context) {
        let Some(b) = &self.busy else { return };
        if b.hidden {
            return;
        }
        let now = ctx.input(|i| i.time);
        let tip = b.tips[((now - b.started) / 3.8) as usize % b.tips.len()];
        let slow = now - b.started > 25.0;
        let title = b.title.clone();
        let mut hide = false;
        modal_frame(ctx, &self.pal, "busy", 460.0, |ui| {
            ui.vertical_centered(|ui| {
                self.mascot(ui, 150.0, Mood::Thinking);
                ui.label(RichText::new(&title).font(bold(24.0)));
                ui.add_space(4.0);
                ui.add(egui::Spinner::new().size(22.0).color(self.pal.primary2));
                ui.add_space(8.0);
                ui.label(RichText::new(tip).color(self.pal.muted));
                if slow {
                    ui.add_space(10.0);
                    ui.label(RichText::new("Still working: big textbooks and busy free AIs can take a minute. If one AI is slow or out of quota, Lumi switches to the next one automatically.").size(13.0).color(self.pal.faint));
                    if widgets::button_sized(ui, "Hide · keep working in the background", self.pal.surface2, true, 12.0, 0.0).clicked() {
                        hide = true;
                    }
                }
            });
        });
        if hide {
            if let Some(b) = &mut self.busy {
                b.hidden = true;
            }
            self.toast("⏳", "Still working on it", "Lumi will let you know when it's ready.", 1);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    fn rewards_ui(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() || self.busy.as_ref().map(|b| !b.hidden).unwrap_or(false) {
            return;
        }
        let Some(r) = self.rewards.front() else { return };
        let p = self.pal;
        let mut close = false;
        let (stage_key, hat) = (self.dash.stage_key.clone(), self.dash.hat.clone());
        let goal = self.dash.goal;
        modal_frame(ctx, &p, "reward", 440.0, |ui| {
            ui.vertical_centered(|ui| match r {
                Reward::Goal => {
                    widgets::mascot_widget(ui, 130.0, Mood::Celebrate, &stage_key, hat.as_deref());
                    ui.label(RichText::new("Daily goal reached! 🎯").font(bold(26.0)));
                    ui.label(RichText::new(format!("You hit {goal} XP today. Your future self says thanks. +10 💎")).color(p.muted));
                    ui.add_space(10.0);
                    close = widgets::button_sized(ui, "Keep going", p.primary, true, 15.0, 300.0).clicked();
                }
                Reward::LevelUp(l) => {
                    widgets::mascot_widget(ui, 120.0, Mood::Celebrate, &stage_key, hat.as_deref());
                    ui.label(RichText::new("LEVEL UP!").font(bold(14.0)).color(p.muted));
                    ui.label(RichText::new(l.to_string()).font(bold(80.0)).color(p.gold));
                    ui.label(format!("You're now level {l}. Every level is proof you showed up."));
                    ui.label(RichText::new("+20 💎").color(p.muted));
                    ui.add_space(10.0);
                    close = widgets::button_sized(ui, "Awesome", p.gold, true, 15.0, 300.0).clicked();
                }
                Reward::Evolved(s) => {
                    ui.label(RichText::new("WHAT'S THIS?!").font(bold(14.0)).color(p.muted));
                    ui.horizontal(|ui| {
                        ui.add_space(40.0);
                        let prev = match s.key { "glow" => "spark", "lumen" => "glow", _ => "lumen" };
                        widgets::mascot_widget(ui, 110.0, Mood::Idle, prev, hat.as_deref());
                        ui.label(RichText::new("→").size(34.0));
                        widgets::mascot_widget(ui, 150.0, Mood::Celebrate, s.key, hat.as_deref());
                    });
                    ui.label(RichText::new(format!("Lumi evolved into {}!", s.name)).font(bold(26.0)));
                    ui.label(RichText::new(match (s.next_name, s.next_level) {
                        (Some(_), Some(l)) => format!("Your learning made Lumi grow. Next evolution at level {l}."),
                        _ => "This is Lumi's final form. Legendary.".into(),
                    }).color(p.muted));
                    ui.add_space(10.0);
                    close = widgets::button_sized(ui, "Amazing!", p.gold, true, 15.0, 300.0).clicked();
                }
            });
        });
        let first_frame = ctx.data_mut(|d| {
            let shown: &mut bool = d.get_temp_mut_or_default(egui::Id::unique("reward_shown"));
            let f = !*shown;
            *shown = true;
            f
        });
        if first_frame {
            match r {
                Reward::Goal => self.sound.play("complete"),
                _ => self.sound.play("levelup"),
            }
            self.confetti(if matches!(r, Reward::Goal) { 120 } else { 220 });
        }
        if close {
            self.rewards.pop_front();
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("reward_shown"), false));
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialog.take() else { return };
        let keep = match &mut d {
            Dialog::Welcome => misc::welcome(self, ctx),
            Dialog::NewCourse { name, goal, emoji } => courses::new_course_dialog(self, ctx, name, goal, emoji),
            Dialog::Note { course, title, text } => courses::note_dialog(self, ctx, *course, title, text),
            Dialog::Confirm { title, body, action } => self.confirm_dialog(ctx, title, body, action),
            Dialog::NewTest(nt) => tests::new_test_dialog(self, ctx, nt),
            Dialog::EditTest { id, name, date, topics } => tests::edit_dialog(self, ctx, *id, name, date, topics),
            Dialog::ConceptMenu { course, concept } => courses::concept_menu(self, ctx, *course, *concept),
            Dialog::BossIntro { course, unit } => courses::boss_intro(self, ctx, *course, *unit),
            Dialog::Chest { gems, freeze, opened_at } => home::chest_dialog(self, ctx, *gems, *freeze, *opened_at),
            Dialog::Image(name) => {
                let mut keep = true;
                let tex = self.chat_textures.get(name).cloned();
                modal_frame(ctx, &self.pal, "img", 720.0, |ui| {
                    if let Some(t) = tex {
                        let s = t.size_vec2();
                        let k = (680.0 / s.x).min(560.0 / s.y).min(4.0);
                        ui.image((t.id(), s * k));
                    }
                    if widgets::button_sized(ui, "Close", self.pal.surface2, true, 13.0, 0.0).clicked() {
                        keep = false;
                    }
                });
                keep
            }
        };
        if keep && self.dialog.is_none() {
            self.dialog = Some(d);
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context, title: &str, body: &str, action: &Confirm) -> bool {
        let mut keep = true;
        let p = self.pal;
        modal_frame(ctx, &p, "confirm", 440.0, |ui| {
            ui.label(RichText::new(title).font(bold(24.0)));
            ui.label(RichText::new(body).color(p.muted));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if widgets::button(ui, "Cancel", p.surface2, true).clicked() {
                    keep = false;
                }
                let label = if matches!(action, Confirm::QuitQuiz) { "Quit" } else { "Delete" };
                if widgets::button(ui, label, p.red, true).clicked() {
                    keep = false;
                    match action {
                        Confirm::DeleteCourse(id) => {
                            crate::actions::delete_course(&self.db, *id);
                            self.go(Route::Courses);
                        }
                        Confirm::DeleteTest(id) => {
                            crate::actions::delete_test(&self.db, *id);
                            self.go(Route::Tests);
                        }
                        Confirm::QuitQuiz => quiz::close(self),
                    }
                }
            });
        });
        keep
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}

pub fn nav_item(ui: &mut Ui, p: &Pal, icon: &str, label: &str, active: bool, badge: Option<i64>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if active {
        ui.painter().rect(rect, 14, widgets::lerp(p.bg, p.primary, 0.18), egui::Stroke::new(2.0, widgets::lerp(p.bg, p.primary, 0.6)), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 14, p.surface);
    }
    ui.painter().text(rect.left_center() + vec2(26.0, 0.0), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(19.0), p.text);
    ui.painter().text(rect.left_center() + vec2(48.0, 0.0), egui::Align2::LEFT_CENTER, label, bold(15.0), if active { p.text } else { p.muted });
    if let Some(n) = badge {
        let c = rect.right_center() - vec2(22.0, 0.0);
        ui.painter().rect_filled(egui::Rect::from_center_size(c, vec2(28.0, 18.0)), 9, p.pink);
        ui.painter().text(c, egui::Align2::CENTER_CENTER, n.to_string(), bold(12.0), Color32::WHITE);
    }
    resp
}

/// A centered column of at most `max_w` width.
pub fn column(ui: &mut Ui, max_w: f32, add: impl FnOnce(&mut Ui)) {
    let avail = ui.available_width();
    let w = avail.min(max_w);
    let pad = ((avail - w) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(w);
            add(ui);
        });
    });
}

/// A dimmed backdrop with a centered card. Used for dialogs and celebrations.
pub fn modal_frame(ctx: &egui::Context, p: &Pal, id: &str, width: f32, add: impl FnOnce(&mut Ui)) -> egui::Response {
    egui::Modal::new(egui::Id::unique(id))
        .backdrop_color(Color32::from_black_alpha(185))
        .frame(egui::Frame::new().fill(p.surface).stroke(egui::Stroke::new(2.0, p.line)).corner_radius(24).inner_margin(26))
        .show(ctx, |ui| {
            ui.set_width(width);
            // min_scrolled_height: the area is first placed from a guessed size, which would otherwise cap the height
            let h = ctx.content_rect().height() * 0.85;
            egui::ScrollArea::vertical().max_height(h).min_scrolled_height(h).show(ui, |ui| add(ui));
        })
        .response
}

pub fn heading(ui: &mut Ui, p: &Pal, text: &str, sub: &str) {
    ui.label(RichText::new(text).font(bold(32.0)).color(p.text));
    if !sub.is_empty() {
        ui.label(RichText::new(sub).color(p.muted));
    }
    ui.add_space(12.0);
}

pub fn section(ui: &mut Ui, p: &Pal, text: &str) {
    ui.add_space(18.0);
    ui.label(RichText::new(text).font(bold(21.0)).color(p.text));
    ui.add_space(6.0);
}

/// Lay out `n` items in rows of up to `cols` equal-width cells (fewer when the window is narrow).
pub fn column_grid(ui: &mut Ui, cols: usize, gap: f32, n: usize, mut f: impl FnMut(&mut Ui, usize, f32)) {
    let avail = ui.available_width();
    let cols = cols.min(((avail + gap) / (210.0 + gap)).floor().max(1.0) as usize).max(1);
    let w = ((avail - gap * (cols - 1) as f32) / cols as f32).floor();
    let idx: Vec<usize> = (0..n).collect();
    for (ri, row) in idx.chunks(cols).enumerate() {
        if ri > 0 {
            ui.add_space(gap);
        }
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &i in row {
                ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(w);
                    f(ui, i, w)
                });
            }
        });
    }
}
