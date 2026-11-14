# Settings feature 4: extension-sections

## What it adds

An extension declares its settings section through its handle. The section's key must be the extension's name. So an extension cannot take the section of an engine, such as `theme`, or the section of another extension.

## How it works

- **Through the handle.** An extension calls `ext.register_settings::<T>(cx)` in its `init`. The handle checks that `T::KEY` is the extension's name, then registers the section with the settings engine, exactly as an engine registers its own: the values come from `settings.json`, or from the defaults, and the section is reloaded when the file changes.
- **Another key is refused.** If `T::KEY` is not the extension's name, nothing is registered, and an `ERROR` names the extension and the key. The extension keeps running. A section that an engine or another extension registered under that key keeps its value.
- **Engines** keep registering their sections with `riso_settings::register`, as they do now. An extension never calls `riso_settings::register` itself: it declares its section only through its handle.
- **Reading** a section does not change: any code reads it with `riso_settings::get::<T>` and observes it with `riso_settings::observe::<T>`.
- **One section per extension.** A second `register_settings` with the extension's key is refused by the engine as today: `section <key> is already registered`.

## What code can do

```rust
// riso_shell
impl Extension {
    /// Registers the section `T`, whose key must be this extension's name.
    pub fn register_settings<T: riso_settings::Settings>(&self, cx: &mut App);
}
```

This is the only new public item. `riso_shell` depends on `riso_settings` with `default-features = false`, and its `dev` feature turns on `riso_settings/dev`.

## Logging

| Situation | Log |
|---|---|
| The key is the extension's name | What the engine logs: `settings-register{section}` with `registered`, inside `extension-init` |
| Another key | `ERROR`, inside `extension-init`: `extension <name> cannot declare the section <key>`. No `settings-register` group opens |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` | Dev version beside production: `Extension::register_settings` |

The settings engine does not change, and the startup does not change.

## Tests

- GPUI tests, after `riso_settings::init` and `load` with a file written in a temporary directory:
  - an extension named `tour` registers a section whose key is `tour`: `get` gives the file's values; without the file's section, its defaults;
  - an extension named `guest` registers that same type, whose key is `tour`: refused; when `tour` registers it afterwards, it is registered and gives the file's values, so the refusal took nothing;
  - an engine registers a section `theme` with `riso_settings::register`, then an extension named `other` registers a type whose key is `theme`: refused, and the engine's value is unchanged;
  - the same extension registering its section twice: the second is refused by the engine, and the first value stays.
- A logging test alone in its file: `settings-register{section=tour}` with `registered` for the accepted one; for each refused one, the `ERROR` with the exact message and no `settings-register` group.
- **Existing tests:** none changes.

## Test extension: `examples/settings_tour`

Its cases `card` and `twice` declare the tour's section through the handle, instead of with `riso_settings::register`; what they show does not change. A second extension of the tour, `settings_guest`, listed after `settings_tour` in `review.rs`, has one case: `SETTINGS_TOUR=guest` makes it declare the tour's section type, whose key is `settings_tour`, through its own handle. Use a fresh home: `RISO_HOME=/tmp/riso-settings`.

| Command | What to look for |
|---|---|
| `SETTINGS_TOUR=card cargo run dev` | The card with the three defaults, as before. `settings-register{section=settings_tour}` inside `extension-init{extension=settings_tour}`. |
| `SETTINGS_TOUR=guest cargo run dev` | The card, as with `card`. Inside `extension-init{extension=settings_guest}`, the `ERROR` `extension settings_guest cannot declare the section settings_tour`, and no second `settings-register`. |
| `SETTINGS_TOUR=twice cargo run dev` | As before: the second `settings-register{section=settings_tour}` fails with `section settings_tour is already registered`. |

## Under review

- `riso_shell/shell`, in its dev version

## Not in this feature

- Stopping an extension from calling `riso_settings::register` itself: a rule of the platform, checked when an extension's dependencies can be checked.
- A section of several extensions, or of a group of extensions.
