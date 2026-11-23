# north_star

A second application built from YARD, beside the sample: its own `product.toml` (North Star, `test.yard.northstar`, scheme `northstar`), its own icon, a one-line `build.rs`, and the startup of `yard_app`. It checks that an application built from YARD shares nothing with the sample. It is a test fixture until `cargo xtask new-app` makes the same application, and then it is deleted.

Run it with a temporary home, so that nothing touches your real files:

```sh
export NORTH_STAR_HOME=$(mktemp -d)
NORTH_STAR_LIFECYCLE=info,north_star=debug cargo run -p north_star
```

| What to look for | Where |
|---|---|
| The window's title is `North Star` | the window |
| The first menu has `About North Star`; choosing it logs `about North Star` in an `extension-action{extension=north_star, …}` group | the menu, the trace |
| The trace starts with `run-start North Star 0.1.0 test.yard.northstar` | the trace |
| The files are `$NORTH_STAR_HOME/logs/North Star.log` and `lifecycle.jsonl`, and `data/` | the home folder |
| `NORTH_STAR_LIFECYCLE`, not `YARD_LIFECYCLE`, sets the trace's filter | the trace |

Run unbundled on macOS, the menu bar shows the executable's name, `north_star`, as the sample shows `yard`; a bundle shows the product's name.
