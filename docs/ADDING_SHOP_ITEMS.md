# Adding shop items

Everything in Lumi's gem shop comes from one file: [`assets/shop.json`](../assets/shop.json).
To add a hat, theme or power-up, add an entry there and rebuild. You don't need to change any Rust code
for hats or themes.

```sh
cargo test --release --lib shop       # checks the catalog (ids, colors, shapes)
cargo test --release --test screens   # renders every screen, including the shop, to target/screens/
cargo run --release                   # try it for real
```

## Rules that keep updates safe

- **Never change or remove an existing `id`.** Players' purchases are saved by id. Rename the
  `name` freely, but the id must stay the same forever.
- Ids are lowercase, unique and short (`"pirate"`, `"space_theme"`).
- `price` is in gems and must be positive. For scale, a player earns roughly 30–60 gems a day: the daily
  chest gives 15–40, hitting the daily goal 10, a level-up 20, and a boss's first defeat 25.
- `icon` is one emoji, shown on the shop card.

## Hats

A hat is a list of shapes drawn on top of Lumi. Lumi lives on a **108 × 108 grid**: x runs left to right,
y runs top to bottom, the body's center is around (54, 60), and the top of the head is around y = 28.
Shapes are drawn in order, so later shapes go on top.

```json
{ "id": "pirate", "kind": "hat", "name": "Pirate Hat", "price": 90, "icon": "🏴‍☠️", "hides_antenna": true,
  "desc": "Arr!",
  "shapes": [
    { "dome": [54, 34, 34, 22], "color": "#1b1b24" },
    { "rect": [18, 30, 72, 7, 3], "color": "#2b2b38" },
    { "circle": [54, 22, 5], "color": "#efe9ff" }
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

Each shape has exactly one of the fields above, plus `"color": "#rrggbb"`. `width` is optional and
defaults to 2. Coordinates can go slightly outside the grid (for example, a tall wizard hat uses y = −8).

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

Power-ups can be bought more than once. The only one so far is `freeze` (streak freeze). A new power-up
needs a few lines of Rust to say what it does: see `buy` in [`src/game.rs`](../src/game.rs).

## Shipping it

1. Add the item, run the tests, commit.
2. Bump `version` in `Cargo.toml` (for example `0.1.0` → `0.2.0`).
3. Tag and push: `git tag v0.2.0 && git push origin main v0.2.0`.

GitHub Actions builds Linux (`.tar.gz` and `.deb`), Windows and macOS downloads and attaches them to a
new release. People's courses, gems and purchases carry over, because their data lives outside the app
(see the README).
