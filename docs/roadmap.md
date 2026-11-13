# Platform roadmap

risoapp is a platform: engines on which applications are built. This file lists the engines, says which are built and which are missing, and gives the order in which the missing ones are built. Each engine has a roadmap of its own, a table of small features.

## The engines

| Engine | Crates | What it gives an application | State | Roadmap |
|---|---|---|---|---|
| Product build config | `app/riso/build.rs` | Its name and version, read when it is built | built | `docs/foundations/roadmap.md` |
| Log | `riso_log` | The lifecycle trace and the diagnostic log | built | `docs/foundations/roadmap.md` |
| Files | `riso_paths` | Its directories, the names of its files, and writing a file in one step | built | `docs/foundations/roadmap.md` |
| Startup | `app/riso` | The order of a start: the log, the files, the extensions, the window | built | `docs/foundations/roadmap.md` |
| Layout | `riso_layout`, `riso_shell` | A tree of components that extensions fill and the user rearranges, saved between runs | first part built, second part planned | `docs/layout-engine/roadmap.md` |
| Settings | `riso_settings` | Everything that can be configured: declared in sections, read from one file, changed while the application runs | reading built: sections, the user's file, live reload. Extension sections and writing are missing | `docs/settings-engine/roadmap.md` |
| Theme | `riso_ui` | The colors, fonts, and density of everything, chosen in the settings | the theme chosen in the settings, switched while running, and following the OS, the UI font, and the density built; more themes and a list of them are missing | `docs/theme-engine/roadmap.md` |
| UI kit | `riso_ui` | The base components everything is drawn with: text, icons, buttons, lists, inputs, menus | tabs and the split's axis built, the rest is missing | `docs/ui-kit/roadmap.md` |
| Keymap | `riso_keymap`, `riso_shell` | The keyboard: focus, actions, default keys, the user's keys, commands by name | missing | `docs/keymap-engine/roadmap.md` |
| Window | `riso_shell`, `riso_ui` | The frame of a window: title bar, status bar, modals, messages, menus. Several windows, remembered | one plain window built, the engine is missing | `docs/window-engine/roadmap.md` |
| State | `riso_state`, `riso_shell` | What the components were showing, kept between runs | missing | `docs/state-engine/roadmap.md` |
| Packaging | tooling beside the workspace | An installable application made from the product config, and its updates | product config built, the rest is missing | `docs/packaging/roadmap.md` |

## How the roadmaps are followed

- Every feature of every roadmap goes through the workflow in `AGENTS.md`, one at a time: a short contract that the user approves, the code in `dev`, a test extension, the lead agent's review, the user's review, the commit, the promotion.
- A feature is small. It can be seen in the running application, through a case of a test extension or in the log.
- The column `Needs` of a feature names what must be in production before it. It can name a feature of another roadmap. So the engines are not built one after another: their features are interleaved, in the order of the next section.
- Nothing is built before something needs it. A feature marked "by need" waits for its first user.

## Order

Each step ends with something an application could not do before. The features inside a step are built in the order given.

| Step | After it, an application can | Features |
|---|---|---|
| 1. Configurable | Read its settings from a file, see them change while it runs, and follow the theme the user or the OS chooses | Settings: `sections`, `user-file`, `live-reload`. Theme: `theme-setting`, `theme-switch`, `system-appearance`, `ui-font`, `density` |
| 2. Base components | Draw its components with text, icons, and buttons that look like Zed's | UI kit: `gallery-and-text`, `icon`, `button` |
| 3. Keyboard | Be used without the mouse: a focused node, actions, default keys | Keymap: `focused-node`, `layout-actions`, `default-keys`, `focus-by-direction`, `extension-actions` |
| 4. A window of its own | Draw its title bar and status bar, put items in them, show a modal, a message, a question, and a menu bar | UI kit: `divider`, `modal`. Window: `title-bar`, `status-bar`, `modal-layer`, `toasts`, `prompts`, `app-menu` |
| 5. Commands and choices | Let the user find every command by name, change the keys, and choose a theme from a list | Settings: `extension-sections`, `write`. UI kit: `list`, `text-input`, `scroll`, `key-hint`. Theme: `user-themes`, `theme-list`. Keymap: `user-keymap`, `keymap-reload`, `command-palette` |
| 6. Layout, second part | Give nodes props and titles, close tabs, size panels in fixed sizes, show a tab per open item, and switch between named layouts | UI kit: `tooltip`. Layout: features 13 to 31 |
| 7. Remembering | Come back as it was left: the windows, what each component showed, the selected tabs | Window: `window-bounds`, `window-tree`, `new-window`, `restore-windows`. State: `node-state`, `save-on-change`, `view-state`, `extension-store` |
| 8. Shipping | Be installed and updated as a product | Packaging: `product-identity`, `bundle-macos`, `open-urls`, `update-check`, `update-install` |

## Reusing Zed

Zed is read before anything is designed, and reused where its license allows. Of the 253 crates of Zed at the pinned revision, 37 are Apache-2.0, 208 are GPL-3.0-or-later, and 8 name no license.

| Code of Zed | License | Rule |
|---|---|---|
| `gpui` and its crates, `util`, `collections`, `sum_tree`, `http_client` | Apache-2.0 | Used directly, anywhere |
| UI crates that stand alone: `ui`, `theme`, `icons`, `menu` | GPL-3.0-or-later | Dependencies of `riso_ui` only, wrapped in riso's own types |
| UI code inside heavy crates: `workspace`, `platform_title_bar`, `picker`, `command_palette` | GPL-3.0-or-later | They cannot be dependencies: they need Zed's project, database, and settings. The pieces needed are copied into `riso_ui` and adapted. Each file keeps Zed's copyright and names the file it comes from |
| Code that is not UI: `settings`, `fs`, `db`, `fuzzy`, `session`, `release_channel`, `auto_update` | GPL-3.0-or-later | Not used and not copied. It is read to understand the problem. riso's code is written on GPUI and on permissively licensed crates |

The test `gpl_boundary` fails when a crate other than `riso_ui` depends on a GPL crate of Zed.

The earlier project, crew, already has a settings store, base components, a text input, and a title bar under the same rules. Its code is ported where a roadmap says so.

## What an extension can do

An extension is a Rust crate compiled into the application. The application's composition root lists the extensions and gives each its name. There is no loading at run time. An extension may depend on another extension's crate and use its types.

| An extension can | Through | State |
|---|---|---|
| Register components, place nodes, and fill slots of other extensions' components | Layout engine | built |
| Change the layout while the application runs, and reset it | Layout engine | built |
| Draw with the theme and the base components | Theme engine, UI kit | partly built |
| Declare its settings | Settings engine: `extension-sections` | missing |
| Declare actions and their default keys | Keymap engine: `extension-actions` | missing |
| Put items in the title bar and the status bar, and show a modal or a message | Window engine | missing |
| Give a model to everything under one of its nodes, or to a whole window | Layout engine: `contexts` | missing |
| Keep the state of its components, and values of its own, between runs | State engine | missing |

Extensions that come with the platform, such as the command palette, are in a folder of their own, `extensions/`. The keymap engine's roadmap adds the first one.

## Not engines of the platform

A project (the folder a window is opened on), a file tree, an editor, a terminal, git, and search are features of an application. An application adds them as extensions.

Zed's crates for these features are GPL and bring most of Zed with them: its `editor` crate alone needs about 96 of Zed's crates. An application builds them on GPUI and on permissively licensed crates.

## Later

These are not in any roadmap yet:

- Turning an extension off without building the application again.
- Translations of the interface.
- Linux and Windows. Everything built so far was run on macOS only.
