# Adding shop items

Everything in Lumi's gem shop comes from one file: [`assets/shop.json`](../assets/shop.json).
To add a hat, accessory, celebration or theme, add an entry there and rebuild. You don't need to change
any Rust code for those.

```sh
cargo test --release --lib shop       # checks the catalog (ids, colors, shapes)
cargo test --release --test screens   # renders every screen to target/screens/; the whole shop is shop_full.png
cargo run --release                   # try it for real
```

## Rules that keep updates safe

- **Never change or remove an existing `id`.** Players' purchases are saved by id. Rename the
  `name` freely, but the id must stay the same forever.
- Ids are lowercase, unique and short (`"space_helmet"`, `"mint"`).
- `price` is in gems and must be positive. For scale, a player earns roughly 30–60 gems a day: the daily
  chest gives 15–40, hitting the daily goal 10, a level-up 20, and a boss's first defeat 25.
- `icon` is one emoji, shown on the shop card.

## Hats and accessories

Lumi wears one hat and one accessory at the same time, so they can be mixed. Accessories (glasses, bow
ties, scarves) are drawn on Lumi's face and body, and hats are drawn on top of everything. Both work the same way:
`"kind": "hat"` or `"kind": "accessory"`, plus a list of shapes.

Useful spots: the eyes are at (42, 52) and (66, 52), the mouth is around (54, 68), the bottom of the body is around y = 90, and the feet are at y = 94.

Lumi lives on a **108 × 108 grid**: x runs left to right,
y runs top to bottom, the body's center is around (54, 60), and the top of the head is around y = 28.
Shapes are drawn in order, so later shapes go on top.

```json
{ "id": "space_helmet", "kind": "hat", "name": "Space Helmet", "price": 90, "icon": "🧑‍🚀", "hides_antenna": true,
  "desc": "To the moon",
  "shapes": [
    { "ring": [54, 58, 40, 38], "width": 4, "color": "#d8e4ff" },
    { "rect": [36, 14, 36, 6, 3], "color": "#d8e4ff" },
    { "circle": [54, 12, 4], "color": "#ff6b4a" }
  ] }
```

Set `"hides_antenna": true` for hats that cover the top of the head.

| Shape     | Value                                | What it draws |
|-----------|--------------------------------------|---------------|
| `poly`    | `[[x, y], [x, y], ...]`              | filled convex polygon (triangles, cones, diamonds) |
| `circle`  | `[x, y, radius]`                     | filled circle |
| `ellipse` | `[x, y, rx, ry]`                     | filled ellipse |
| `rect`    | `[x, y, width, height, corner]`      | filled rounded rectangle (x, y is the top-left corner) |
| `line`    | `[[x1, y1], [x2, y2]]` + `width`     | line segment |
| `arc`     | `[cx, cy, rx, ry]` + `width`         | top half of an ellipse outline (headbands) |
| `dome`    | `[cx, cy, rx, ry]`                   | filled top half of an ellipse (beanies, helmets) |
| `star`    | `[x, y, radius]`                     | filled five-point star |
| `ring`    | `[cx, cy, rx, ry]` + `width`         | ellipse outline (glasses, halos, monocles) |

Each shape has exactly one of the fields above, plus `"color": "#rrggbb"`. `width` is optional and
defaults to 2. Coordinates can go slightly outside the grid (for example, a tall wizard hat uses y = −8).
Polygons must be convex (no dents). Build a dented shape out of several convex pieces, the way the
crown and heart glasses do. The catalog test catches dented polygons.

## Celebrations

A celebration changes the confetti that bursts out when you finish a session, level up or hit your daily goal:

```json
{ "id": "leaves", "kind": "celebration", "name": "Autumn Leaves", "price": 50, "icon": "🍂",
  "particle": "ribbon", "palette": ["#e8743b", "#d9a43a", "#b5532c", "#f2c14e"] }
```

`particle` is the shape of each piece: `ribbon`, `circle`, `star` or `heart`. `palette` is a list of colors to pick from at random.

## Themes

A theme recolors the whole app. Copy an existing theme and change the colors:

```json
{ "id": "mint", "kind": "theme", "name": "Mint", "price": 70, "icon": "🌿",
  "colors": { "bg": "#07130f", "surface": "#0d2019", "surface2": "#143026", "surface3": "#1b3f32",
              "line": "#214a3b", "primary": "#2fd39a", "primary2": "#7fe8c2",
              "muted": "#a5d1c0", "faint": "#5f9a85", "accent": "#ffd84d" } }
```

| Key | Used for |
|-----|----------|
| `bg` | window background |
| `surface`, `surface2`, `surface3` | cards, from darkest to lightest (hover and pressed states) |
| `line` | borders |
| `primary`, `primary2` | main brand color and a lighter version (buttons, highlights) |
| `muted`, `faint` | secondary text and hint text |
| `accent` | optional; small highlights |

Every key except `accent` is required (the catalog test checks this). Keep `bg` dark and the text colors
light, because the rest of the palette (green, red and yellow for answers) is tuned for dark themes.

## Power-ups

Power-ups can be bought more than once. You can change their `name`, `price`, `icon` and `desc` in the JSON.

| Id | What it does |
|----|--------------|
| `freeze` | protects the streak for one missed day (up to 2) |
| `double_xp` | 2× XP for 15 minutes (`DOUBLE_XP_MINUTES` in `src/game.rs`); buying again adds time |
| `mystery` | a random hat, accessory, celebration or theme the player doesn't own yet; pricier items are rarer |

A brand-new power-up needs a few lines of Rust to say what it does. Add its id to `POWERUPS` in
[`src/shop.rs`](../src/shop.rs) and handle it in `buy` in [`src/game.rs`](../src/game.rs).

## Shipping it

1. Add the item, run the tests, commit.
2. Bump `version` in `Cargo.toml` (for example `0.1.0` → `0.2.0`).
3. Tag and push: `git tag v0.2.0 && git push origin main v0.2.0`.

GitHub Actions builds Linux (`.tar.gz` and `.deb`), Windows and macOS downloads and attaches them to a
new release. People's courses, gems and purchases carry over, because their data lives outside the app
(see the README).
