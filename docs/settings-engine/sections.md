# Settings feature 1: sections

## What it adds

A new crate, `yard_settings`. Code declares a section: a key and a Rust type whose `Default` gives every value. Any code that has the `App` then reads the section's value. There is no file yet, so every section has its defaults.

## The crate

- `yard_settings` depends on `gpui`, `serde`, `tracing`, and `log`. It depends on no crate of yard and on no crate of Zed but `gpui`.
- Its code is YARD's own, ported from the earlier project's `crew_settings`, which is the user's code. Nothing is copied from Zed's `settings` crate.
- Its first part is `yard_settings/sections`, in `src/dev/sections.rs`.

## What code can do

```rust
pub trait Settings: serde::de::DeserializeOwned + Default + 'static {
    const KEY: &'static str;
}

pub fn init(cx: &mut App);
pub fn register<T: Settings>(cx: &mut App);
pub fn get<T: Settings>(cx: &App) -> &T;
```

- **A section** is a type that implements `Settings`. `KEY` is its key in the file that the next feature reads. The type deserializes, because the next feature reads its values from JSON. Its `Default` gives every value.
- **`init`** creates the store, once, in the startup's `app-init`, before any extension runs.
- **`register::<T>`** declares the section and publishes its defaults.
- **`get::<T>`** returns the section's value. It is cheap, and can be called while drawing.
- Each section's value is a GPUI global of its own. The store is a second global that holds the keys that are registered.

## Rules

| Situation | What happens | Log |
|---|---|---|
| `register` of a new key | The defaults are published | `settings-register{section}`, `debug`: `registered` |
| `register` of a key that is already registered, by the same type or another one | Refused. The first section and its value stay | `ERROR` in `settings-register{section}`: `section <key> is already registered` |
| `get` of a section that is not registered | Panic: `section <type> is read before it is registered`. It is a bug in the caller, found the first time the code runs | — |
| `register` before `init` | Panic: `yard_settings::init runs before a section is registered` | — |
| `init` a second time | Panic: `yard_settings::init runs once` | — |

- `init` runs in a group, `settings-init`, and logs `store created` at `debug`.
- A refused duplicate is an `ERROR`, not a panic, as a refused component registration is in the shell: the application keeps running with the first section.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_settings/sections` (new, dev) | The trait, `init`, `register`, `get`, and their logging |
| `yard/startup` | Dev copy, `app/yard/src/dev/startup.rs`. It calls `yard_settings::dev::init(cx)` in `app-init`, after `yard_ui::init` and before the extensions run |

`app/yard` depends on `yard_settings` with `default-features = false`, and its `dev` feature turns on `yard_settings/dev`.

## Tests

- Unit tests in `crates/yard_settings/src/dev/sections/tests.rs`, with GPUI's test context:
  - a registered section reads as its `Default`;
  - two sections with different keys are read independently;
  - a second `register` of the same key, by the same type or by another type with the same `KEY`, leaves the first value;
  - `get` before `register`, `register` before `init`, and a second `init` panic with the messages above.
- A logging test alone in `crates/yard_settings/tests/sections_logging.rs`: `settings-init` and `settings-register{section}` are opened, `registered` is at `debug`, and a duplicate logs the `ERROR` inside its `settings-register` group.

## Test extension: `examples/settings_tour`

A new test extension. It declares a section of its own:

| Key | Value | Default |
|---|---|---|
| `settings_tour` | `greeting`, text | `hello` |
| | `size`, a number | `14` |
| | `enabled`, yes or no | `true` |

It registers a component, `settings_tour.card`, and places it as the root. The card draws with the theme, as the cards of `layout_tour` do, and shows the key and one line per value: `greeting: hello`, `size: 14`, `enabled: true`. The values are read with `get` while the card draws.

| Command | What to look for |
|---|---|
| `cargo run dev` | Nothing from the tour. `settings-init` in `app-init`. |
| `SETTINGS_TOUR=card cargo run dev` | The card with the three defaults. In the trace, `settings-register{section=settings_tour}` inside `extension-init{extension=settings_tour}`, then `registered`. |
| `SETTINGS_TOUR=twice cargo run dev` | The tour registers its section a second time. The second `settings-register` fails (✗) with `section settings_tour is already registered`. The card still shows the defaults. |

## Under review

- `yard_settings/sections`
- `yard/startup`, in its dev version

## Not in this feature

- Reading `settings.json`. That is the next feature, `user-file`, which also adds `Paths::settings_file()`.
- Telling the users of a section that it changed, and watching the file: `live-reload`.
- Declaring a section through an extension's handle, with the extension's name as its key: `extension-sections`.
