# Platform roadmap

YARD is a platform: engines on which applications are built. This file lists the engines, says which are built and which are missing, and gives the order in which the missing ones are built. Each engine has a roadmap of its own, a table of small features.

## The engines

| Engine | Crates | What it gives an application | State | Roadmap |
|---|---|---|---|---|
| Product build config | `app/yard/build.rs` | Its name, version, identifier, icon, read when it is built | built | `docs/foundations/roadmap.md` |
| Log | `yard_log` | The lifecycle trace and the diagnostic log | built | `docs/foundations/roadmap.md` |
| Files | `yard_paths` | Its directories, the names of its files, and writing a file in one step | built | `docs/foundations/roadmap.md` |
| Startup | `app/yard` | The order of a start: the log, the files, the extensions, the window | built | `docs/foundations/roadmap.md` |
| Layout | `yard_layout`, `yard_shell` | A tree of components that extensions fill and the user rearranges, saved between runs | built | `docs/layout-engine/roadmap.md` |
| Settings | `yard_settings` | Everything that can be configured: declared in sections, read from one file, changed while the application runs | built: sections, the user's file, live reload, extension sections, and writing a value back | `docs/settings-engine/roadmap.md` |
| Theme | `yard_ui` | The colors, fonts, and density of everything, chosen in the settings | the theme chosen in the settings, switched while running, and following the OS, the UI font, the density, the user's theme files, and a list of the themes to choose from built | `docs/theme-engine/roadmap.md` |
| UI kit | `yard_ui` | The base components everything is drawn with: text, icons, buttons, lists, inputs, menus | tabs, the split's axis, text, the row and column helpers, icons, buttons, dividers, the modal frame, lists, a text input, scrolling, and key hints built, with the gallery; the rest is missing | `docs/ui-kit/roadmap.md` |
| Keymap | `yard_keymap`, `yard_shell` | The keyboard: focus, actions, default keys, the user's keys, commands by name | the focused node, the shell's actions on it, moving the focus by direction, their default keys, and extensions' own actions and keys, the user's keys, reloaded when their file changes, and the command palette, the first extension in `extensions/`, built | `docs/keymap-engine/roadmap.md` |
| Window | `yard_shell`, `yard_ui` | The frame of a window: title bar, status bar, modals, messages, menus. Several windows, remembered | built: the frame, title bar, status bar, modal layer, toasts, questions, menu bar, and several windows, each remembered with its place, size, and layout | `docs/window-engine/roadmap.md` |
| State | `yard_state`, `yard_shell` | What the components were showing, kept between runs | built: each node's state, saved soon after a change, the selected tabs and the zoom, and each extension's own values | `docs/state-engine/roadmap.md` |
| Packaging | tooling beside the workspace | An installable application made from the product config, and its updates | built: the product's identity, the macOS bundle, opening URLs, and checking for and installing updates | `docs/packaging/roadmap.md` |
| Platform completeness | tooling, the engines | What a desktop platform is commonly expected to give and YARD does not yet: CI, signing, signed updates, file dialogs, notifications, crash reports, Linux and Windows | planned | `docs/platform/roadmap.md` |

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
| UI crates that stand alone: `ui`, `theme`, `icons`, `menu` | GPL-3.0-or-later | Dependencies of `yard_ui` only, wrapped in YARD's own types |
| UI code inside heavy crates: `workspace`, `platform_title_bar`, `picker`, `command_palette` | GPL-3.0-or-later | They cannot be dependencies: they need Zed's project, database, and settings. The pieces needed are copied into `yard_ui` and adapted. Each file keeps Zed's copyright and names the file it comes from |
| Code that is not UI: `settings`, `fs`, `db`, `fuzzy`, `session`, `release_channel`, `auto_update` | GPL-3.0-or-later | Not used and not copied. It is read to understand the problem. YARD's code is written on GPUI and on permissively licensed crates |

The test `gpl_boundary` fails when a crate other than `yard_ui` depends on a GPL crate of Zed.

The earlier project, crew, already has a settings store, base components, a text input, and a title bar under the same rules. Its code is ported where a roadmap says so.

## What an extension can do

An extension is a Rust crate compiled into the application. The application's composition root lists the extensions and gives each its name. There is no loading at run time. An extension may depend on another extension's crate and use its types.

| An extension can | Through | State |
|---|---|---|
| Register components, place nodes, and fill slots of other extensions' components | Layout engine | built |
| Change the layout while the application runs, and reset it | Layout engine | built |
| Draw with the theme and the base components | Theme engine, UI kit | built |
| Declare its settings | Settings engine: `extension-sections` | built |
| Declare actions and their default keys | Keymap engine: `extension-actions` | built |
| Put items in the title bar and the status bar, and show a modal or a message | Window engine | built: the title bar, the status bar, modals, toasts, questions, and the menu bar |
| Give a model to everything under one of its nodes, or to a whole window | Layout engine: `contexts` | built |
| Keep the state of its components, and values of its own, between runs | State engine | built |
| Handle URLs of the product's scheme that name it | Packaging: `open-urls` | built |

Extensions that come with the platform, such as the command palette, are in a folder of their own, `extensions/`.

## Not engines of the platform

A project (the folder a window is opened on), a file tree, an editor, a terminal, git, and search are features of an application. An application adds them as extensions.

Zed's crates for these features are GPL and bring most of Zed with them: its `editor` crate alone needs about 96 of Zed's crates. An application builds them on GPUI and on permissively licensed crates.

## Later

These are not in any roadmap yet:

- Turning an extension off without building the application again.
- Translations of the interface.
- Linux and Windows. Everything built so far was run on macOS only.
