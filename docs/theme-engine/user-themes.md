# Theme feature 6: user-themes

## What it adds

Theme files in Zed's format, put by the user in the folder `themes` of the config directory, are registered at startup beside the bundled One themes, as Zed loads the files of its `themes` folder. The user then names one of them in `settings.json`, and the window takes it.

## How it works

As Zed's `theme_settings::load_user_theme` and its folder of user themes:

- **The folder.** `riso_paths` names it: `config/themes`, beside `settings.json`. It may not exist; then nothing is loaded and nothing is wrong.
- **The files.** Every file whose name ends in `.json` in that folder, not in its subfolders, in the order of their names. Each is a theme family in Zed's format, as Zed's extensions ship them: a `name`, an `author`, and `themes`, each theme with a `name`, an `appearance` (`light` or `dark`), and a `style`. A family may hold several themes; each is registered under its own name.
- **Read as the bundled themes are read,** by `riso_ui`'s reader of Zed's theme files, so the same colors come out as Zed's.
- **A file that cannot be read** is a `WARN` that names it and says why; the other files still load. A file that is not `.json` is skipped without a word.
- **The same name twice.** A user theme with the name of a bundled theme, or of a theme from an earlier file, replaces it, as Zed's registry does.
- **When.** Once, at startup, in `app-init`, before the theme setting is applied, so a user theme named in `settings.json` is active from the first frame. A file added while riso runs is read at the next start: watching the folder is not part of this feature.
- **The setting.** Unchanged: the theme section names a dark and a light theme and a mode, as the earlier features decided: `{"theme": {"mode": "dark", "dark": "Ayu Dark"}}`. A name that is not registered keeps the active theme with a `WARN`, as today.

## What code can do

```rust
// riso_paths
impl Paths {
    /// The folder of the user's theme files.
    pub fn themes_dir(&self) -> PathBuf;
}

// riso_ui
/// Registers every theme file of `dir`. Returns how many themes were registered.
pub fn load_user_themes(cx: &mut App, dir: &Path) -> usize;
```

These are the only new public items.

## Logging

| Situation | Log |
|---|---|
| The startup loads the folder | A group `themes-load{path}` inside `app-init` |
| A file is read | Inside it, `debug`: `theme file loaded` with `file` and `themes`, the number of themes it holds |
| A file cannot be read | Inside it, `WARN`: `the theme file <file> cannot be read: <reason>` |
| The folder does not exist | Inside it, `debug`: `no themes folder`, and nothing more |
| The end, when the folder exists | Inside it, `debug`: `user themes loaded` with `count`, the number of themes registered |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_paths/themes_dir` (new, dev) | `Paths::themes_dir` |
| `riso_ui/user_themes` (new, dev) | `load_user_themes`, using the reader of `riso_ui/theming` |
| `riso/startup` | Dev version beside production: `riso_ui::load_user_themes(cx, &paths.themes_dir())` in `app-init`, after `riso_ui::init` and before `riso_ui::apply_theme_setting` |
| `riso_ui/theming` (production, changed and stamped again) | Its reader of one theme file, `family`, becomes `pub(crate)`, so that `user_themes` reads with it. And the reader no longer stops the application when a theme's `style` lacks `players` or `syntax`, which Zed's format allows: a missing key gives the defaults, as in Zed. Every existing test passes unchanged |

`user_themes` parses each file as JSON itself first, so that its `WARN` can say why a file is not valid JSON; a file that is JSON but not a theme family is reported as `not a theme family in Zed's format`.

## Tests

- A plain test: `themes_dir` is `config/themes` under the home.
- GPUI tests, with a folder in a temporary directory:
  - a file holding Zed's Ayu family (its three themes) registers `Ayu Dark`, `Ayu Light`, and `Ayu Mirage`; `load_user_themes` returns 3;
  - two files, one broken: the good one's themes are registered, and the count is the good one's;
  - a file that is not `.json` is skipped;
  - a user theme named `One Dark` replaces the bundled one: the registry's `One Dark` has the user file's background color;
  - a folder that does not exist registers nothing and returns 0;
  - after loading, setting `{"theme": {"mode": "dark", "dark": "Ayu Dark"}}` makes `Ayu Dark` active.
- A logging test alone in its file: every row of the table, with exact messages and fields.
- **Existing tests:** none changes.

## Test extension

The theme engine's features are shown through the settings: `examples/settings_tour`'s card, or any case, in the theme the settings name. Zed's Ayu file is copied into the tour's `fixtures/`, with its license, for the user to copy.

| Command and action | What to look for |
|---|---|
| Copy `examples/settings_tour/fixtures/ayu.json` to `$RISO_HOME/config/themes/`, write `{"theme": {"mode": "dark", "dark": "Ayu Dark"}}`, `SETTINGS_TOUR=card cargo run dev` | The window in Ayu Dark from the first frame. `themes-load` with `theme file loaded {file=ayu.json, themes=3}`, `user themes loaded {count=3}`. |
| Change the setting to `{"theme": {"mode": "light", "light": "Ayu Light"}}` while it runs | The window switches, as the theme setting already does. |
| Put a broken copy beside it, `broken.json`, and start again | The `WARN` names `broken.json`; Ayu still loads. |
| Remove the folder and start | `no themes folder`; the window in One Dark, with the `WARN` that `Ayu Light` is not registered. |

## Under review

- `riso_paths/themes_dir`, new
- `riso_ui/user_themes`, new
- `riso/startup`, in its dev version

## Not in this feature

- Watching the folder, so that a theme file added or changed while riso runs is read at once: with the `keymap-reload` pattern, by need.
- Icon themes, and themes installed by extensions.
