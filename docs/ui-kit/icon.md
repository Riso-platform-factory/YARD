# UI kit feature 2: icon

## What it adds

An icon by name, in Zed's sizes and colors, drawn from SVG files that `yard_ui` bundles. The gallery gets a page `icon` with every bundled icon and its name.

## What code can do

All are YARD's own types in `yard_ui`:

```rust
pub enum IconName { Check, ChevronDown, ChevronRight, Close, Info, Plus, Warning, XCircle }
pub enum IconSize { Indicator, XSmall, Small, Medium, XLarge }
pub struct Icon;            // Icon::new(IconName).size(IconSize).color(TextColor)
pub struct Assets;          // the bundled files, given to GPUI by the startup
impl IconName { pub const ALL: &[IconName]; pub fn path(self) -> &'static str; }
```

- `Icon` wraps Zed's `ui::Icon`. Its defaults are `IconSize::Medium` and `TextColor::Default`, the same color names as a label's.
- **The sizes are Zed's,** in rems:

  | Size | At 16px |
  |---|---|
  | `Indicator` | 10px |
  | `XSmall` | 12px |
  | `Small` | 14px |
  | `Medium` | 16px |
  | `XLarge` | 48px |

- **The set is small and grows by need.** It holds the icons that the next features of the roadmaps use: the modal's and the toasts' `Close`, the toasts' `Info`, `Warning`, and `XCircle`, the lists' and the theme list's `Check`, the tree's and the menus' `ChevronDown` and `ChevronRight`, and the tabs' `Plus`. Each name maps to Zed's icon of the same meaning. Adding an icon is one name and one copied file.
- **Each icon is a file of Zed's,** copied unchanged into `crates/yard_ui/assets/icons/`. Beside them are:
  - `LICENSES`, copied from Zed: most of Zed's icons are Lucide's, under the ISC license;
  - a `NOTICE` that names Zed's revision.

  Zed's file does not say which icons are Lucide's and which Zed drew itself, which are GPL-3.0-or-later like yard. Both may be copied.
- **`Assets` gives GPUI the bundled files,** by path, such as `icons/close.svg`. GPUI takes one source of files when the application is made, so the startup gives it `Assets`. An icon whose file is missing draws nothing, as in GPUI.
- Drawing an icon logs one line at `trace`, with its name, size, and color. Loading a file that is not bundled logs a `WARN` with its path.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/icon` (new, dev) | `Icon`, `IconName`, `IconSize`, and `Assets`, with the copied files and their licenses |
| `yard/startup` | Dev copy. It gives GPUI `yard_ui::dev::Assets` when it makes the application |

## Tests

- Unit tests:
  - every `IconName` has its file in `Assets`, and the file is an SVG;
  - `Assets` lists every icon under `icons/`;
  - a path that is not bundled gives nothing;
  - every `IconSize` gives Zed's size.
- GPUI tests with `Assets`: every icon at every size and in a few colors draws in a test window.
- A logging test alone in its file: drawing an icon logs one `trace` line; loading a path that is not bundled logs the `WARN`.

## Test extension: `examples/ui_gallery`

A second page, `icon`, after `text`:

- a row per `IconSize`, each with every icon at that size;
- a row of `Check` in each `TextColor`;
- then a list of every icon with its name beside it.

Use a fresh home: `YARD_HOME=/tmp/yard-gallery`.

| Command | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev` | Two tabs, `text` and `icon`. On `icon`, every icon at every size, the colors, and the names. `gallery-build{pages=2}`. |
| The same, in One Light | The icons follow the theme's colors. |

## Under review

- `yard_ui/icon`
- `yard/startup`, in its dev version

## Not in this feature

- Icons that extensions bring with them: a second source of files, when an extension first needs it.
- File type icons from an icon theme.
