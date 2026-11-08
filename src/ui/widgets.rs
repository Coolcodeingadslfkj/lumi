//! Drawing helpers: chunky buttons, cards, progress bars, Lumi the mascot, bosses, charts, confetti.

use super::theme::Pal;
use egui::{epaint::QuadraticBezierShape, pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

pub fn hex(s: &str) -> Color32 {
    let s = s.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("80"), 16).unwrap_or(128);
    Color32::from_rgb(p(0), p(2), p(4))
}

pub fn darken(c: Color32, f: f32) -> Color32 {
    Color32::from_rgb((c.r() as f32 * f) as u8, (c.g() as f32 * f) as u8, (c.b() as f32 * f) as u8)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("bold".into()))
}

/// A chunky 3D button (Duolingo style). Returns the response; `.clicked()` as usual.
pub fn button(ui: &mut Ui, text: &str, color: Color32, enabled: bool) -> Response {
    button_sized(ui, text, color, enabled, 15.0, 0.0)
}

pub fn button_sized(ui: &mut Ui, text: &str, color: Color32, enabled: bool, size: f32, min_w: f32) -> Response {
    // dark text on bright buttons (gold, light theme colors), white otherwise
    let luma = 0.299 * color.r() as f32 + 0.587 * color.g() as f32 + 0.114 * color.b() as f32;
    let text_color = if !enabled { Color32::from_gray(120) } else if luma > 170.0 { darken(color, 0.25) } else { Color32::WHITE };
    let galley = ui.painter().layout_no_wrap(text.to_uppercase(), bold(size), text_color);
    let pad = vec2(if size < 14.0 { 14.0 } else { 22.0 }, if size < 14.0 { 8.0 } else { 12.0 });
    let size_v = vec2((galley.size().x + pad.x * 2.0).max(min_w), galley.size().y + pad.y * 2.0 + 4.0);
    let (rect, resp) = ui.allocate_exact_size(size_v, if enabled { Sense::click() } else { Sense::hover() });
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    let fill = if !enabled { Color32::from_rgb(42, 46, 92) } else if resp.hovered() { lighten(color, 1.08) } else { color };
    let down = enabled && resp.is_pointer_button_down_on();
    let body = Rect::from_min_size(rect.min + vec2(0.0, if down { 4.0 } else { 0.0 }), vec2(rect.width(), rect.height() - 4.0));
    let r = CornerRadius::same(if size < 14.0 { 12 } else { 16 });
    if !down {
        ui.painter().rect_filled(body.translate(vec2(0.0, 4.0)), r, darken(fill, 0.72));
    }
    ui.painter().rect_filled(body, r, fill);
    ui.painter().galley(body.center() - galley.size() / 2.0, galley, text_color);
    resp
}

pub fn lighten(c: Color32, f: f32) -> Color32 {
    let l = |v: u8| ((v as f32 * f).min(255.0)) as u8;
    Color32::from_rgb(l(c.r()), l(c.g()), l(c.b()))
}

/// Rounded card frame.
pub fn card(p: &Pal) -> egui::Frame {
    egui::Frame::new().fill(p.surface).stroke(Stroke::new(2.0, p.line)).corner_radius(18).inner_margin(18)
}

pub fn card_glow(p: &Pal) -> egui::Frame {
    egui::Frame::new().fill(lerp(p.surface, p.primary, 0.16)).stroke(Stroke::new(2.0, lerp(p.line, p.primary, 0.6))).corner_radius(18).inner_margin(18)
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

pub fn progress(ui: &mut Ui, frac: f32, color: Color32, height: f32, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let r = CornerRadius::same((height / 2.0) as u8);
    ui.painter().rect_filled(rect, r, Color32::from_rgb(42, 46, 92));
    let f = frac.clamp(0.0, 1.0);
    if f > 0.0 {
        let fill = Rect::from_min_size(rect.min, vec2((rect.width() * f).max(height), height));
        ui.painter().rect_filled(fill, r, color);
        if height >= 10.0 {
            let shine = Rect::from_min_size(fill.min + vec2(height / 2.0, 3.0), vec2((fill.width() - height).max(0.0), 3.0));
            ui.painter().rect_filled(shine, 2, Color32::from_white_alpha(70));
        }
    }
}

pub fn chip(ui: &mut Ui, p: &Pal, text: &str, color: Color32) -> Response {
    egui::Frame::new().fill(p.surface).stroke(Stroke::new(2.0, p.line)).corner_radius(99).inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| ui.label(egui::RichText::new(text).font(bold(15.0)).color(color)))
        .response
}

pub fn tag(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new().fill(bg).corner_radius(99).inner_margin(egui::Margin::symmetric(9, 2))
        .show(ui, |ui| ui.label(egui::RichText::new(text).font(bold(12.0)).color(fg)));
}

/// Daily-goal style ring with a label in the middle.
pub fn ring(ui: &mut Ui, p: &Pal, size: f32, frac: f32, big: &str, small: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let c = rect.center();
    let r = size / 2.0 - 7.0;
    let w = (size / 9.0).max(6.0);
    ui.painter().circle_stroke(c, r, Stroke::new(w, p.surface3));
    let f = frac.clamp(0.0, 1.0);
    if f > 0.001 {
        let n = (64.0 * f).max(2.0) as usize;
        let pts: Vec<Pos2> = (0..=n).map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * f * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        }).collect();
        ui.painter().add(Shape::line(pts, Stroke::new(w, color)));
    }
    ui.painter().text(c - vec2(0.0, 7.0), Align2::CENTER_CENTER, big, bold(size / 5.0), p.text);
    ui.painter().text(c + vec2(0.0, size / 8.0), Align2::CENTER_CENTER, small, bold((size / 10.0).max(10.0)), p.muted);
}

// ---------- Lumi the mascot ----------

#[derive(Clone, Copy, PartialEq)]
pub enum Mood {
    Idle,
    Happy,
    Thinking,
    Sad,
    Celebrate,
}

fn ellipse_pts(c: Pos2, rx: f32, ry: f32, rot: f32, n: usize) -> Vec<Pos2> {
    let (s, co) = rot.sin_cos();
    (0..n).map(|i| {
        let a = std::f32::consts::TAU * i as f32 / n as f32;
        let (x, y) = (a.cos() * rx, a.sin() * ry);
        c + vec2(x * co - y * s, x * s + y * co)
    }).collect()
}

/// Draw Lumi into `rect`. Stage: spark | glow | lumen | sage. Hat: shop item id.
/// Lumi the mascot. `hat` and `acc` are shop item ids (see assets/shop.json).
pub fn mascot(ui: &Ui, rect: Rect, mood: Mood, stage: &str, hat: Option<&str>, acc: Option<&str>, t: f32) {
    let k = rect.width().min(rect.height()) / 108.0;
    let bob = match mood {
        Mood::Happy | Mood::Celebrate => -(t * 9.0).sin().abs() * 8.0,
        _ => (t * 2.0).sin() * 3.0,
    };
    let o = rect.center() - vec2(54.0, 54.0) * k + vec2(0.0, bob * k);
    let p = |x: f32, y: f32| o + vec2(x, y) * k;
    let pt = ui.painter();
    let purple = Color32::from_rgb(155, 130, 255);
    let deep = Color32::from_rgb(106, 77, 255);
    let ink = Color32::from_rgb(27, 20, 64);
    let gold = Color32::from_rgb(255, 216, 77);
    let glow = (t * 2.6).sin() * 0.3 + 0.7;
    let hides_antenna = hat.and_then(crate::shop::get).map(|h| h.hides_antenna).unwrap_or(false);
    if mood == Mood::Celebrate || stage == "sage" {
        pt.text(p(10.0, 16.0), Align2::CENTER_CENTER, "✨", FontId::proportional(14.0 * k), Color32::WHITE);
        pt.text(p(96.0, 24.0), Align2::CENTER_CENTER, "⭐", FontId::proportional(12.0 * k), Color32::WHITE);
    }
    if stage == "lumen" || stage == "sage" {
        pt.add(Shape::closed_line(ellipse_pts(p(54.0, 60.0), 44.0 * k, 41.0 * k, 0.0, 48), Stroke::new(2.5 * k, Color32::from_rgba_unmultiplied(255, 230, 128, (140.0 * glow) as u8))));
    }
    if !hides_antenna {
        let (tip, r) = if stage == "spark" { (p(54.0, 21.0), 4.0) } else { (p(54.0, 14.0), 6.0) };
        pt.circle_filled(tip, (if stage == "spark" { 10.0 } else { 16.0 }) * k, Color32::from_rgba_unmultiplied(255, 230, 128, (60.0 * glow) as u8));
        pt.add(QuadraticBezierShape::from_points_stroke([p(54.0, 30.0), p(50.0, 22.0), tip], false, Color32::TRANSPARENT, Stroke::new(4.0 * k, purple)));
        if stage == "lumen" || stage == "sage" {
            star(pt, tip, 8.0 * k, gold);
        } else {
            pt.circle_filled(tip, r * k, gold);
        }
    }
    if stage != "spark" {
        pt.add(Shape::convex_polygon(ellipse_pts(p(22.0, 60.0), 9.0 * k, 14.0 * k, -0.44, 24), Color32::from_rgb(138, 109, 255), Stroke::NONE));
        pt.add(Shape::convex_polygon(ellipse_pts(p(86.0, 60.0), 9.0 * k, 14.0 * k, 0.44, 24), Color32::from_rgb(138, 109, 255), Stroke::NONE));
    }
    // body with a soft highlight (stand-in for the gradient)
    pt.add(Shape::ellipse_filled(p(54.0, 60.0), vec2(34.0, 32.0) * k, deep));
    pt.add(Shape::ellipse_filled(p(52.0, 57.0), vec2(31.0, 28.0) * k, purple));
    pt.add(Shape::ellipse_filled(p(46.0, 48.0), vec2(16.0, 12.0) * k, Color32::from_rgb(190, 176, 255)));
    pt.add(Shape::ellipse_filled(p(54.0, 70.0), vec2(20.0, 14.0) * k, Color32::from_rgba_unmultiplied(239, 233, 255, 120)));
    let cheek = if stage == "spark" { vec2(6.5, 4.0) } else { vec2(5.0, 3.0) };
    pt.add(Shape::ellipse_filled(p(32.0, 64.0), cheek * k, Color32::from_rgba_unmultiplied(255, 143, 181, 170)));
    pt.add(Shape::ellipse_filled(p(76.0, 64.0), cheek * k, Color32::from_rgba_unmultiplied(255, 143, 181, 170)));
    // eyes
    let blink = (t % 4.5) > 4.35;
    match mood {
        Mood::Happy | Mood::Celebrate => {
            for x in [36.0, 60.0] {
                pt.add(QuadraticBezierShape::from_points_stroke([p(x, 52.0), p(x + 6.0, 42.0), p(x + 12.0, 52.0)], false, Color32::TRANSPARENT, Stroke::new(5.0 * k, ink)));
            }
        }
        Mood::Sad => {
            pt.add(Shape::ellipse_filled(p(42.0, 54.0), vec2(6.0, 7.0) * k, ink));
            pt.add(Shape::ellipse_filled(p(66.0, 54.0), vec2(6.0, 7.0) * k, ink));
            pt.line_segment([p(33.0, 46.0), p(45.0, 42.0)], Stroke::new(4.0 * k, ink));
            pt.line_segment([p(75.0, 46.0), p(63.0, 42.0)], Stroke::new(4.0 * k, ink));
        }
        _ => {
            let ey = if blink { 1.0 } else { 9.0 };
            let look = if mood == Mood::Thinking { -2.0 } else { 0.0 };
            for x in [42.0, 66.0] {
                pt.add(Shape::ellipse_filled(p(x, 52.0), vec2(7.0, ey) * k, ink));
                if !blink {
                    pt.circle_filled(p(x + 2.0, 49.0 + look), 2.6 * k, Color32::WHITE);
                }
            }
        }
    }
    if stage == "sage" {
        for x in [42.0, 66.0] {
            pt.circle_stroke(p(x, 52.0), 10.0 * k, Stroke::new(2.5 * k, ink));
        }
        pt.line_segment([p(52.0, 52.0), p(56.0, 52.0)], Stroke::new(2.5 * k, ink));
    }
    // mouth
    match mood {
        Mood::Sad => { pt.add(QuadraticBezierShape::from_points_stroke([p(47.0, 72.0), p(54.0, 66.0), p(61.0, 72.0)], false, Color32::TRANSPARENT, Stroke::new(4.0 * k, ink))); }
        Mood::Thinking => { pt.circle_filled(p(54.0, 71.0), 3.5 * k, ink); }
        Mood::Celebrate => { pt.add(QuadraticBezierShape::from_points_stroke([p(45.0, 66.0), p(54.0, 80.0), p(63.0, 66.0)], true, Color32::from_rgb(255, 107, 154), Stroke::new(4.0 * k, ink))); }
        _ => { pt.add(QuadraticBezierShape::from_points_stroke([p(45.0, 66.0), p(54.0, 75.0), p(63.0, 66.0)], false, Color32::TRANSPARENT, Stroke::new(4.0 * k, ink))); }
    }
    pt.add(Shape::ellipse_filled(p(42.0, 94.0), vec2(8.0, 4.0) * k, deep));
    pt.add(Shape::ellipse_filled(p(66.0, 94.0), vec2(8.0, 4.0) * k, deep));
    // accessories sit on the face/body, hats on top of everything
    for id in [acc, hat].into_iter().flatten() {
        draw_item(pt, id, &p, k);
    }
}

fn star(pt: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let pts: Vec<Pos2> = (0..10).map(|i| {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 5.0;
        let rr = if i % 2 == 0 { r } else { r * 0.45 };
        c + vec2(a.cos(), a.sin()) * rr
    }).collect();
    // star isn't convex: fan of triangles from the center
    for i in 0..10 {
        pt.add(Shape::convex_polygon(vec![c, pts[i], pts[(i + 1) % 10]], color, Stroke::NONE));
    }
}

/// Draw a shop hat or accessory from its shapes in assets/shop.json.
fn draw_item(pt: &egui::Painter, id: &str, p: &dyn Fn(f32, f32) -> Pos2, k: f32) {
    let Some(item) = crate::shop::get(id) else { return };
    for s in &item.shapes {
        let c = hex(&s.color);
        if let Some(pts) = &s.poly {
            pt.add(Shape::convex_polygon(pts.iter().map(|q| p(q[0], q[1])).collect(), c, Stroke::NONE));
        } else if let Some([x, y, r]) = s.circle {
            pt.circle_filled(p(x, y), r * k, c);
        } else if let Some([x, y, rx, ry]) = s.ellipse {
            pt.add(Shape::ellipse_filled(p(x, y), vec2(rx, ry) * k, c));
        } else if let Some([x, y, w, h, r]) = s.rect {
            pt.rect_filled(Rect::from_min_size(p(x, y), vec2(w, h) * k), CornerRadius::same((r * k) as u8), c);
        } else if let Some([a, b]) = s.line {
            pt.line_segment([p(a[0], a[1]), p(b[0], b[1])], Stroke::new(s.width * k, c));
        } else if let Some([cx, cy, rx, ry]) = s.arc {
            let pts: Vec<Pos2> = (0..=24).map(|i| { let a = std::f32::consts::PI * i as f32 / 24.0; p(cx - a.cos() * rx, cy - a.sin() * ry) }).collect();
            pt.add(Shape::line(pts, Stroke::new(s.width * k, c)));
        } else if let Some([cx, cy, rx, ry]) = s.dome {
            let pts: Vec<Pos2> = (0..=20).map(|i| { let a = std::f32::consts::PI * i as f32 / 20.0; p(cx - a.cos() * rx, cy - a.sin() * ry) }).collect();
            pt.add(Shape::convex_polygon(pts, c, Stroke::NONE));
        } else if let Some([x, y, r]) = s.star {
            star(pt, p(x, y), r * k, c);
        } else if let Some([cx, cy, rx, ry]) = s.ring {
            pt.add(Shape::closed_line(ellipse_pts(p(cx, cy), rx * k, ry * k, 0.0, 32), Stroke::new(s.width * k, c)));
        }
    }
}

pub fn mascot_widget(ui: &mut Ui, size: f32, mood: Mood, stage: &str, hat: Option<&str>, acc: Option<&str>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    mascot(ui, rect, mood, stage, hat, acc, t);
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    resp
}

// ---------- bosses ----------

#[derive(Clone, Copy, PartialEq)]
pub enum BossMood {
    Idle,
    Hurt,
    Defeated,
}

pub fn boss(ui: &Ui, rect: Rect, color: Color32, mood: BossMood) {
    let k = rect.width().min(rect.height()) / 108.0;
    let o = rect.center() - vec2(54.0, 54.0) * k;
    let p = |x: f32, y: f32| o + vec2(x, y) * k;
    let pt = ui.painter();
    let horn = Color32::from_rgb(239, 233, 255);
    pt.add(Shape::convex_polygon(vec![p(30.0, 34.0), p(20.0, 6.0), p(44.0, 26.0)], horn, Stroke::NONE));
    pt.add(Shape::convex_polygon(vec![p(78.0, 34.0), p(88.0, 6.0), p(64.0, 26.0)], horn, Stroke::NONE));
    pt.add(Shape::ellipse_filled(p(54.0, 62.0), vec2(40.0, 37.0) * k, darken(color, 0.55)));
    pt.add(Shape::ellipse_filled(p(52.0, 58.0), vec2(36.0, 33.0) * k, color));
    pt.add(Shape::ellipse_filled(p(44.0, 46.0), vec2(14.0, 9.0) * k, lighten(color, 1.25)));
    let white = Color32::WHITE;
    let ink = Color32::from_rgb(27, 20, 64);
    match mood {
        BossMood::Hurt => {
            for x in [34.0, 62.0] {
                pt.line_segment([p(x, 50.0), p(x + 12.0, 58.0)], Stroke::new(4.0 * k, white));
                pt.line_segment([p(x + 12.0, 50.0), p(x, 58.0)], Stroke::new(4.0 * k, white));
            }
        }
        BossMood::Defeated => {
            for x in [34.0, 62.0] {
                pt.add(QuadraticBezierShape::from_points_stroke([p(x, 54.0), p(x + 6.0, 48.0), p(x + 12.0, 54.0)], false, Color32::TRANSPARENT, Stroke::new(4.0 * k, white)));
            }
        }
        BossMood::Idle => {
            pt.line_segment([p(30.0, 44.0), p(48.0, 51.0)], Stroke::new(5.0 * k, ink));
            pt.line_segment([p(78.0, 44.0), p(60.0, 51.0)], Stroke::new(5.0 * k, ink));
            for x in [41.0, 67.0] {
                pt.circle_filled(p(x, 56.0), 6.0 * k, white);
                pt.circle_filled(p(x + if x < 50.0 { 1.0 } else { -1.0 }, 57.0), 3.0 * k, ink);
            }
        }
    }
    let teeth: Vec<Pos2> = [36.0, 42.0, 48.0, 54.0, 60.0, 66.0, 72.0].iter().enumerate().map(|(i, x)| p(*x, if i % 2 == 0 { 78.0 } else { 72.0 })).collect();
    pt.add(Shape::line(teeth, Stroke::new(3.5 * k, white)));
}

pub const BOSSES: [&str; 8] = ["Gloom Golem", "Quiz Kraken", "Doubt Dragon", "Fog Phantom", "Muddle Minotaur", "Riddle Cyclops", "Blur Basilisk", "Static Sphinx"];
pub const UNIT_COLORS: [&str; 6] = ["#7c5cff", "#ff6b9a", "#22a8c5", "#22c58b", "#ff8a3d", "#c04fe0"];

pub fn boss_name(course_id: i64, unit: usize) -> &'static str {
    BOSSES[(course_id as usize * 3 + unit) % BOSSES.len()]
}

// ---------- confetti & floating text ----------

pub struct Particle {
    pos: Pos2,
    vel: Vec2,
    rot: f32,
    spin: f32,
    size: f32,
    color: Color32,
    shape: u8,
}

impl Particle {
    const RIBBON: u8 = 0;
    const CIRCLE: u8 = 1;
    const STAR: u8 = 2;
    const HEART: u8 = 3;

    fn draw(&self, pt: &egui::Painter) {
        let (s, c) = self.rot.sin_cos();
        match self.shape {
            Self::CIRCLE => {
                pt.circle_filled(self.pos, self.size / 2.0, self.color);
            }
            Self::STAR => star(pt, self.pos, self.size / 1.6, self.color),
            Self::HEART => {
                // two lobes plus a point, tilted with the spin
                let rot = |v: Vec2| vec2(v.x * c - v.y * s, v.x * s + v.y * c) * (self.size / 10.0);
                let r = self.size * 0.28;
                pt.circle_filled(self.pos + rot(vec2(-2.4, -1.5)), r, self.color);
                pt.circle_filled(self.pos + rot(vec2(2.4, -1.5)), r, self.color);
                pt.add(Shape::convex_polygon(vec![self.pos + rot(vec2(-5.1, -0.4)), self.pos + rot(vec2(5.1, -0.4)), self.pos + rot(vec2(0.0, 5.5))], self.color, Stroke::NONE));
            }
            _ => {
                let hw = vec2(c, s) * self.size / 2.0;
                let hh = vec2(-s, c) * self.size / 4.0;
                pt.add(Shape::convex_polygon(vec![self.pos - hw - hh, self.pos + hw - hh, self.pos + hw + hh, self.pos - hw + hh], self.color, Stroke::NONE));
            }
        }
    }
}

fn particle_kind(name: Option<&str>) -> u8 {
    match name {
        Some("circle") => Particle::CIRCLE,
        Some("star") => Particle::STAR,
        Some("heart") => Particle::HEART,
        _ => Particle::RIBBON,
    }
}

/// A still sample of a confetti style, for the shop card.
pub fn confetti_preview(ui: &mut Ui, size: Vec2, style: Option<&crate::shop::Item>) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let classic = ["#7c5cff", "#ff6b9a", "#22c58b", "#ffc542", "#6fd3ff", "#ff8a3d"];
    let colors: Vec<&str> = match style {
        Some(s) if !s.palette.is_empty() => s.palette.iter().map(|c| c.as_str()).collect(),
        _ => classic.to_vec(),
    };
    let shape = particle_kind(style.and_then(|s| s.particle.as_deref()));
    let t = ui.input(|i| i.time) as f32;
    // fixed layout so the card doesn't flicker; a gentle sway keeps it alive
    let spots = [(0.12, 0.25), (0.3, 0.7), (0.45, 0.2), (0.62, 0.55), (0.8, 0.3), (0.9, 0.78), (0.2, 0.5), (0.55, 0.85), (0.72, 0.12), (0.38, 0.45)];
    for (i, (x, y)) in spots.iter().enumerate() {
        let sway = (t * 1.5 + i as f32).sin() * 3.0;
        Particle {
            pos: rect.min + vec2(x * rect.width() + sway, y * rect.height()),
            vel: Vec2::ZERO,
            rot: i as f32 * 0.9 + t * 0.6,
            spin: 0.0,
            size: if shape == Particle::RIBBON { 13.0 } else { 15.0 },
            color: hex(colors[i % colors.len()]),
            shape,
        }
        .draw(ui.painter());
    }
}

#[derive(Default)]
pub struct Fx {
    particles: Vec<Particle>,
    floats: Vec<(String, Pos2, f64, Color32)>,
}

impl Fx {
    /// Burst of confetti; `style` is an equipped "celebration" shop item (None = classic).
    pub fn confetti(&mut self, screen: Rect, n: usize, style: Option<&crate::shop::Item>) {
        use rand::RngExt;
        let mut r = rand::rng();
        let classic = ["#7c5cff", "#ff6b9a", "#22c58b", "#ffc542", "#6fd3ff", "#ff8a3d"];
        let colors: Vec<&str> = match style {
            Some(s) if !s.palette.is_empty() => s.palette.iter().map(|c| c.as_str()).collect(),
            _ => classic.to_vec(),
        };
        let shape = particle_kind(style.and_then(|s| s.particle.as_deref()));
        for _ in 0..n {
            self.particles.push(Particle {
                pos: pos2(screen.center().x + r.random_range(-100.0..100.0), screen.top() + screen.height() * 0.35),
                vel: vec2(r.random_range(-8.0..8.0), -r.random_range(4.0..17.0)),
                rot: r.random_range(0.0..6.0),
                spin: r.random_range(-0.3..0.3),
                size: r.random_range(5.0..12.0) * if shape == Particle::RIBBON { 1.0 } else { 1.25 },
                color: hex(colors[r.random_range(0..colors.len())]),
                shape,
            });
        }
    }

    pub fn float(&mut self, text: impl Into<String>, at: Pos2, now: f64, color: Color32) {
        self.floats.push((text.into(), at, now, color));
    }

    pub fn paint(&mut self, ctx: &egui::Context) {
        if self.particles.is_empty() && self.floats.is_empty() {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("fx")));
        let screen = ctx.content_rect();
        let now = ctx.input(|i| i.time);
        for p in &mut self.particles {
            p.vel.y += 0.35;
            p.vel.x *= 0.99;
            p.pos += p.vel;
            p.rot += p.spin;
            p.draw(&painter);
        }
        self.particles.retain(|p| p.pos.y < screen.bottom() + 40.0);
        self.floats.retain(|(text, at, t0, color)| {
            let age = (now - t0) as f32;
            if age > 1.1 {
                return false;
            }
            let a = (1.0 - age / 1.1).clamp(0.0, 1.0);
            let c = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (255.0 * a) as u8);
            painter.text(*at - vec2(0.0, 70.0 * age), Align2::CENTER_CENTER, text, bold(26.0 + age * 4.0), c);
            true
        });
        ctx.request_repaint();
    }
}
