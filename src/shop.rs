//! Shop catalog, loaded from `assets/shop.json` (compiled into the app).
//! Adding a hat, theme or power-up only needs a new entry there: see docs/ADDING_SHOP_ITEMS.md.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Deserialize, Clone, Debug)]
pub struct Item {
    pub id: String,
    /// "hat", "accessory", "celebration", "theme" or "powerup"
    pub kind: String,
    pub name: String,
    pub price: i64,
    pub icon: String,
    #[serde(default)]
    pub desc: String,
    /// Hats and accessories: shapes drawn on Lumi (108x108 grid).
    #[serde(default)]
    pub shapes: Vec<Shape>,
    /// Hats that sit on top of Lumi's head hide the antenna.
    #[serde(default)]
    pub hides_antenna: bool,
    /// Themes: colors as "#rrggbb".
    #[serde(default)]
    pub colors: HashMap<String, String>,
    /// Celebrations: confetti colors as "#rrggbb".
    #[serde(default)]
    pub palette: Vec<String>,
    /// Celebrations: confetti piece shape: "ribbon", "circle", "star" or "heart".
    #[serde(default)]
    pub particle: Option<String>,
}

/// Wearable/equippable kinds; each has its own slot (the setting with the same name).
pub const SLOTS: &[&str] = &["hat", "accessory", "celebration", "theme"];
/// Power-ups with behavior in game::buy.
pub const POWERUPS: &[&str] = &["freeze", "double_xp", "mystery"];
pub const PARTICLES: &[&str] = &["ribbon", "circle", "star", "heart"];

/// One drawing primitive. Exactly one geometry field is set, plus a color.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Shape {
    /// Filled convex polygon: [[x, y], ...]
    pub poly: Option<Vec<[f32; 2]>>,
    /// Filled circle: [x, y, r]
    pub circle: Option<[f32; 3]>,
    /// Filled ellipse: [x, y, rx, ry]
    pub ellipse: Option<[f32; 4]>,
    /// Rounded rectangle: [x, y, w, h, corner_radius]
    pub rect: Option<[f32; 5]>,
    /// Line: [[x1, y1], [x2, y2]] with `width`
    pub line: Option<[[f32; 2]; 2]>,
    /// Upper half-ellipse outline (headband): [cx, cy, rx, ry] with `width`
    pub arc: Option<[f32; 4]>,
    /// Filled upper half-ellipse (a dome, like a beanie): [cx, cy, rx, ry]
    pub dome: Option<[f32; 4]>,
    /// Filled five-point star: [x, y, r]
    pub star: Option<[f32; 3]>,
    /// Ellipse outline (glasses, halos): [cx, cy, rx, ry] with `width`
    pub ring: Option<[f32; 4]>,
    #[serde(default = "default_width")]
    pub width: f32,
    pub color: String,
}

fn default_width() -> f32 {
    2.0
}

#[derive(Deserialize)]
struct Catalog {
    items: Vec<Item>,
}

pub static ITEMS: LazyLock<Vec<Item>> = LazyLock::new(|| {
    let c: Catalog = serde_json::from_str(include_str!("../assets/shop.json")).expect("assets/shop.json is not valid (see docs/ADDING_SHOP_ITEMS.md)");
    c.items
});

pub fn items() -> &'static [Item] {
    &ITEMS
}

pub fn get(id: &str) -> Option<&'static Item> {
    ITEMS.iter().find(|i| i.id == id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalog_is_valid() {
        use super::{PARTICLES, POWERUPS, SLOTS};
        let items = super::items();
        let color_ok = |c: &str| c.starts_with('#') && c.len() == 7 && u32::from_str_radix(&c[1..], 16).is_ok();
        for p in POWERUPS {
            assert!(items.iter().any(|i| i.id == *p), "power-up {p} must exist");
        }
        let mut ids = std::collections::HashSet::new();
        for i in items {
            assert!(ids.insert(&i.id), "duplicate shop id {}", i.id);
            assert!(SLOTS.contains(&i.kind.as_str()) || i.kind == "powerup", "{}: kind must be one of {SLOTS:?} or powerup", i.id);
            assert!(i.price > 0, "{}: price must be positive", i.id);
            if i.kind == "powerup" {
                assert!(POWERUPS.contains(&i.id.as_str()), "{}: new power-ups need code in game::buy (see docs)", i.id);
            }
            if i.kind == "hat" || i.kind == "accessory" {
                assert!(!i.shapes.is_empty(), "{}: needs shapes", i.id);
                for s in &i.shapes {
                    let n = [s.poly.is_some(), s.circle.is_some(), s.ellipse.is_some(), s.rect.is_some(), s.line.is_some(), s.arc.is_some(), s.dome.is_some(), s.star.is_some(), s.ring.is_some()].iter().filter(|x| **x).count();
                    assert_eq!(n, 1, "{}: each shape needs exactly one geometry (poly, circle, ...)", i.id);
                    assert!(color_ok(&s.color), "{}: colors look like #rrggbb", i.id);
                    if let Some(pts) = &s.poly {
                        assert!(pts.len() >= 3 && is_convex(pts), "{}: polygons must be convex, with 3+ points", i.id);
                    }
                }
            }
            if i.kind == "theme" {
                for k in ["bg", "surface", "surface2", "surface3", "line", "primary", "primary2", "muted", "faint"] {
                    assert!(i.colors.get(k).is_some_and(|c| color_ok(c)), "{}: theme needs color {k} as #rrggbb", i.id);
                }
            }
            if i.kind == "celebration" {
                assert!(!i.palette.is_empty() && i.palette.iter().all(|c| color_ok(c)), "{}: palette needs #rrggbb colors", i.id);
                assert!(i.particle.as_deref().is_some_and(|p| PARTICLES.contains(&p)), "{}: particle must be one of {PARTICLES:?}", i.id);
            }
        }
    }

    fn is_convex(pts: &[[f32; 2]]) -> bool {
        let n = pts.len();
        let mut sign = 0.0f32;
        for i in 0..n {
            let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
            let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
            if cross.abs() > 1e-3 {
                if sign != 0.0 && cross.signum() != sign {
                    return false;
                }
                sign = cross.signum();
            }
        }
        true
    }
}
