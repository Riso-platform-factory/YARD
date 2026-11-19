# keys_tour

A test extension for the keymap engine. It declares GPUI actions of its own, binds their keys through its handle, and handles them in two ways: in its component, and for the whole window. When `app/yard/src/dev/review.rs` lists it as `("keys_tour", keys_tour::init)` among the extensions it passes to the startup, and `app/yard/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. Every feature it covers is in production, so it is not loaded now; add both back to check the keymap engine again, or when the next keymap feature is under review.

Without `KEYS_TOUR` it does nothing. With `KEYS_TOUR=all`, it places a row of two cards, `a` and `b`. A card shows its node, its count, the pings, and `focused` when its node has the focus.

| Action | Key | Context | Effect |
|---|---|---|---|
| `keys_tour::Increment` | `ctrl-alt-i` | `keys_tour` | The focused card's count goes up by one |
| `keys_tour::Ping` | `ctrl-alt-p` | `Shell` | Logs `ping`; both cards show one more ping |

Use a fresh home, so that the user's real files are never touched: `YARD_HOME=/tmp/yard-keys`.

## Keymap feature 5: extension-actions

| Command and action | What to look for |
|---|---|
| `KEYS_TOUR=all cargo run dev` | Inside `extension-init{extension=keys_tour}`: `keys-bind{extension=keys_tour, count=2}` with a `bound` line per key, and `action handled {extension=keys_tour, action=keys_tour::Ping}`. |
| `ctrl-alt-i` before clicking anything | Nothing: no card has the focus. |
| Click card `a`, then `ctrl-alt-i` twice | `a` counts 2; `b` stays 0. `incremented {node=a, count=2}`. |
| Click card `b`, then `ctrl-alt-i` | `b` counts 1. |
| `ctrl-alt-p`, with or without a focused card | `extension-action{extension=keys_tour, action=keys_tour::Ping}` with `ping` inside; both cards show `pings: 1`. |
| `shift-escape` on a focused card | The shell's zoom still works. |

## Removing it

Remove it from the list of extensions in `app/yard/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/yard/Cargo.toml`, and the app runs as it should without it.
