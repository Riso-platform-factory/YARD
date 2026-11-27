# Roadmap

YARD is organized into focused engines. The initial eight steps of the engine roadmaps are in production, and the `packaging` and `new-app` slices of the platform roadmap are implemented. This page summarizes the engines and lists every remaining planned, by-need, or later item.

Platform-level capabilities (upstream updates, CI, multi-platform updates, file dialogs, OS notifications, crash reports) are tracked in [Platform Roadmap](./platform-roadmap.md).

## The Engines

| Engine | Crates | Role | State |
|---|---|---|---|
| Product build config | `yard_build_config`, `yard_product` | Product identity (`name`, `version`, `identifier`, `url_scheme`, `icon`, `update_url`, `[release]`), build script, and derived names | production |
| Log | `yard_log` | Diagnostic log (`<Name>.log`) and lifecycle trace (`lifecycle.jsonl`) | production |
| Files | `yard_paths` | Platform directories, `<NAME>_HOME`, file names, and atomic writes | production |
| Startup | `yard_app` | Initialization sequence: logs, single instance, engines, extensions, session restore, updates; the sample `app/yard` keeps only its composition (application value and extensions) | production |
| Layout | `yard_layout`, `yard_shell` | Per-window component tree, splits, tabs, drag-and-drop, sizing, snap-hide, contexts, item slots, named layouts, user layers | production |
| Settings | `yard_settings` | Typed JSON sections in `settings.json`, extension sections, live reload, atomic writes | production |
| Theme | `yard_ui` | Colors, system appearance, user themes, UI font, UI density, theme list | production (code font waits by need) |
| UI kit | `yard_ui` | Base components wrapped from Zed's `ui` and GPUI, plus `examples/ui_gallery` | core components in production; additional components by need |
| Keymap | `yard_keymap`, `yard_shell`, `extensions/command_palette` | Focus, directional focus, actions, default keys, extension actions, `keymap.json` with live reload, command palette | production |
| Window | `yard_shell`, `yard_ui` | Frame (title bar, body, status bar), drawn controls on Linux and Windows, modals, toasts, prompts, menus, multiple windows, bounds and session restore | production |
| State | `yard_state`, `yard_shell` | Per-node state (`state.json`), debounced saves, tab/zoom view state, per-extension key-value store | production (`secrets` and `item-state` wait by need) |
| Packaging, scaffolding, and updates | `tooling/xtask`, `yard_update`, `yard_shell` | `cargo xtask new-app`, `cargo xtask bundle` (macOS, Linux, Windows, signing), custom URL schemes, single instance, macOS update check and install | `yard_update` and URL handling in production; `xtask` scaffolding and bundling in `dev` |

Not engines of the platform: projects, file trees, editors, terminals, git, and search belong to applications built on YARD as extensions.

## Features Waiting by Need

Features marked "by need" wait for their first real user before being designed and built.

### Theme Engine

| Slug | What it adds | Needs / waits for |
|---|---|---|
| `code-font` | Bundles Lilex as the code font, exposes its family and size in the `theme` section, and passes it to Zed's `ui` components (removes `TODO(code-font)`) | UI kit: `code-text` |

### UI Kit

| Slug | What it adds | Reference |
|---|---|---|
| `context-menu` | Right-click and popover menus | Zed `ContextMenu`, `PopoverMenu` |
| `toggle` | Checkbox and switch controls | Zed `Checkbox`, `Switch` |
| `callout` | Inline severity message inside a view | Zed `Callout`, `Banner` |
| `tree-item` | Collapsible tree row with indent guides | Zed `TreeViewItem`, `Disclosure`, `IndentGuides` |
| `code-text` | Read-only text rendered in the code font (first user of `code-font`) | YARD on GPUI text |
| `text-area` | Multi-line text input with wrapping, scrolling, and undo | `gpui/examples/input.rs` |
| `markdown` | Rendered Markdown text | YARD |
| `data-table`, `progress` | Row/column data table and progress bar | Zed data table and progress components |

### State Engine

| Slug | What it adds | Needs / waits for |
|---|---|---|
| `secrets` | Store and retrieve credentials in the OS keychain via the extension handle | First extension needing a password or token |
| `item-state` | Persist state per item-slot copy | Layout engine: `item-slots` (built) |

## Later Ideas (Unscheduled)

Items noted across engine roadmaps that are not yet scheduled:

- **Layout:** single-operation undo; replacing a node's component; wrapping a node; edit mode with outlines and slots; collapsible accordion container; per-extension mutation permissions; migrating older layout file versions; per-project layouts; dragging non-tab nodes; animations.
- **Settings:** comments and trailing commas in `settings.json` (preserving them on write); JSON schema generation for declared sections; per-project override files; key migration on rename; a settings editor extension.
- **Theme:** per-color `theme_overrides` in settings; live reload for user theme files; additional bundled theme families (Ayu, Gruvbox); named syntax colors; file-type icon themes.
- **Keymap:** base keymaps for other editors and Vim modes; keymap editor UI; key-chord continuation popup; non-US keyboard layouts; keyboard commands to resize splits and move nodes.
- **Window:** prompting components before closing a window with unsaved work; a shell toggle-fullscreen action and menu item (title bar chrome already adapts to fullscreen and saved bounds restore fullscreen state); macOS window tabs; dragging nodes between windows; background progress indicators in the status bar.
- **State:** migrating older state file versions. A queryable database for an application's data is now [Platform Roadmap](./platform-roadmap.md) `data-store`.
- **Packaging and platform:** command-line launcher opening files or folders in the running instance; interface localization/translations; see [Platform Roadmap](./platform-roadmap.md) for upstream updates, CI, multi-platform updates, release channels, file dialogs, OS notifications, crash reports, and toggling compiled-in extensions.
