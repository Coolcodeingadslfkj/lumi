//! Colors (shop themes) and fonts.

use super::widgets::hex;
use egui::{Color32, FontData, FontDefinitions, FontFamily, Stroke};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: Color32,
    pub surface: Color32,
    pub surface2: Color32,
    pub surface3: Color32,
    pub line: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub primary: Color32,
    pub primary2: Color32,
    pub pink: Color32,
    pub green: Color32,
    pub red: Color32,
    pub gold: Color32,
    pub flame: Color32,
    pub ice: Color32,
}

pub fn palette(theme: Option<&str>) -> Pal {
    let base = Pal {
        bg: hex("#0d0e1c"), surface: hex("#171933"), surface2: hex("#20234a"), surface3: hex("#2a2e5c"), line: hex("#2f3366"),
        text: hex("#f1f2ff"), muted: hex("#a3a8d6"), faint: hex("#6d72a8"), primary: hex("#7c5cff"), primary2: hex("#a08bff"),
        pink: hex("#ff6b9a"), green: hex("#22c58b"), red: hex("#ff5c7a"), gold: hex("#ffc542"), flame: hex("#ff8a3d"), ice: hex("#6fd3ff"),
    };
    // themes come from assets/shop.json; missing colors fall back to the default theme
    let Some(item) = theme.and_then(crate::shop::get).filter(|i| i.kind == "theme") else { return base };
    let c = |k: &str, d: Color32| item.colors.get(k).map(|v| hex(v)).unwrap_or(d);
    Pal {
        bg: c("bg", base.bg), surface: c("surface", base.surface), surface2: c("surface2", base.surface2), surface3: c("surface3", base.surface3),
        line: c("line", base.line), primary: c("primary", base.primary), primary2: c("primary2", base.primary2),
        muted: c("muted", base.muted), faint: c("faint", base.faint), text: c("text", base.text), ..base
    }
}

pub fn apply_visuals(ctx: &egui::Context, p: &Pal) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.surface2;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.primary2;
    v.selection.bg_fill = p.primary;
    v.window_stroke = Stroke::new(2.0, p.line);
    v.window_corner_radius = 22.into();
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = 12.into();
    }
    v.widgets.inactive.bg_fill = p.surface2;
    v.widgets.inactive.weak_bg_fill = p.surface2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.5, p.line);
    v.widgets.hovered.bg_fill = p.surface3;
    v.widgets.hovered.weak_bg_fill = p.surface3;
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, p.primary);
    v.widgets.active.bg_fill = p.primary;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        use egui::{FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Heading, FontId::new(28.0, FontFamily::Name("bold".into()))),
            (TextStyle::Body, FontId::new(15.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(12.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(10.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.scroll.bar_width = 8.0;
    });
}

/// Bundled Noto fonts (text, math, symbols) plus the system's color emoji font when there is one.
pub fn fonts(color_emoji: bool) -> FontDefinitions {
    let mut f = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut f, "noto", include_bytes!("../../assets/fonts/NotoSans-Regular.ttf"));
    add(&mut f, "noto-bold", include_bytes!("../../assets/fonts/NotoSans-Bold.ttf"));
    add(&mut f, "noto-math", include_bytes!("../../assets/fonts/NotoSansMath-Regular.ttf"));
    add(&mut f, "noto-sym2", include_bytes!("../../assets/fonts/NotoSansSymbols2-Regular.ttf"));
    let mut emoji = vec![];
    if color_emoji {
        if let Some(bytes) = system_color_emoji() {
            f.font_data.insert("color-emoji".into(), Arc::new(FontData::from_owned(bytes)));
            emoji.push("color-emoji".to_string());
        }
    }
    // egui's own (monochrome) emoji font stays as the fallback
    let defaults = f.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = vec!["noto".to_string()];
    prop.extend(emoji.clone());
    prop.extend(defaults.iter().cloned());
    prop.extend(["noto-math".to_string(), "noto-sym2".to_string()]);
    let mut boldf = vec!["noto-bold".to_string()];
    boldf.extend(prop[1..].iter().cloned());
    let mono_defaults = f.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut mono = mono_defaults;
    mono.extend(emoji);
    mono.extend(["noto-math".to_string(), "noto-sym2".to_string()]);
    f.families.insert(FontFamily::Proportional, prop);
    f.families.insert(FontFamily::Name("bold".into()), boldf);
    f.families.insert(FontFamily::Monospace, mono);
    f
}

fn system_color_emoji() -> Option<Vec<u8>> {
    let candidates: &[&str] = if cfg!(target_os = "windows") {
        &["C:\\Windows\\Fonts\\seguiemj.ttf"]
    } else if cfg!(target_os = "macos") {
        &["/System/Library/Fonts/Apple Color Emoji.ttc"]
    } else {
        &["/usr/share/fonts/noto/NotoColorEmoji.ttf", "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
          "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf", "/usr/share/fonts/noto-emoji/NotoColorEmoji.ttf",
          "/usr/share/fonts/TTF/NotoColorEmoji.ttf", "/usr/local/share/fonts/NotoColorEmoji.ttf"]
    };
    candidates.iter().find_map(|p| std::fs::read(p).ok())
}
