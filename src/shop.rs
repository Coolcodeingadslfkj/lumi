//! Shop catalog, loaded from `assets/shop.json` (compiled into the app).
//! Adding a hat, theme or power-up only needs a new entry there: see docs/ADDING_SHOP_ITEMS.md.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Deserialize, Clone, Debug)]
pub struct Item {
    pub id: String,
    /// "hat", "theme" or "powerup"
    pub kind: String,
    pub name: String,
    pub price: i64,
    pub icon: String,
    #[serde(default)]
    pub desc: String,
    /// Hats: shapes drawn on Lumi (108x108 grid).
    #[serde(default)]
    pub shapes: Vec<Shape>,
    /// Hats that sit on top of Lumi's head hide the antenna.
    #[serde(default)]
    pub hides_antenna: bool,
    /// Themes: colors as "#rrggbb".
    #[serde(default)]
    pub colors: HashMap<String, String>,
}

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
        let items = super::items();
        assert!(items.iter().any(|i| i.id == "freeze"), "the streak freeze power-up must exist");
        let mut ids = std::collections::HashSet::new();
        for i in items {
            assert!(ids.insert(&i.id), "duplicate shop id {}", i.id);
            assert!(["hat", "theme", "powerup"].contains(&i.kind.as_str()), "{}: kind must be hat, theme or powerup", i.id);
            assert!(i.price > 0, "{}: price must be positive", i.id);
            if i.kind == "hat" {
                assert!(!i.shapes.is_empty(), "{}: a hat needs shapes", i.id);
                for s in &i.shapes {
                    let n = [s.poly.is_some(), s.circle.is_some(), s.ellipse.is_some(), s.rect.is_some(), s.line.is_some(), s.arc.is_some(), s.dome.is_some(), s.star.is_some()].iter().filter(|x| **x).count();
                    assert_eq!(n, 1, "{}: each shape needs exactly one geometry (poly, circle, ...)", i.id);
                    assert!(s.color.starts_with('#') && s.color.len() == 7, "{}: colors look like #rrggbb", i.id);
                }
            }
            if i.kind == "theme" {
                for k in ["bg", "surface", "surface2", "surface3", "line", "primary", "primary2", "muted", "faint"] {
                    assert!(i.colors.contains_key(k), "{}: theme is missing color {k}", i.id);
                }
            }
        }
    }
}
