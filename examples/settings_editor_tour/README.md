# Settings editor tour

The settings editor and its schema are in production (promoted after `0796fb6`), so `cargo run -p yard` opens the editor with cmd-, on the built-in **Theme** and **Network** sections. This tour adds a section with one field of each kind. It is not in `app/yard/src/dev/review.rs` any more: to run it, add `("settings_editor_tour", settings_editor_tour::init)` to the extension list there and `settings_editor_tour` to `app/yard/Cargo.toml`, as at `0796fb6`, then use a separate YARD home and start the review build.

```sh
YARD_HOME=/tmp/yard-settings-editor SETTINGS_EDITOR_TOUR=1 cargo run -p yard -- dev
```

The tour registers **Settings editor tour** when `SETTINGS_EDITOR_TOUR=1`. Its files are under `/tmp/yard-settings-editor/config`.

| Action | What to see |
|---|---|
| Press Cmd+, on macOS or Ctrl+, on Linux and Windows. | The **Settings** dialog opens. Select **Settings editor tour** on the left. |
| Turn on **Preview** and choose **date** for **Sort**. Enter **25** for **Page size** and press Enter. Change **Greeting**, then click the **Page size** input to leave **Greeting**. | `settings.json` saves each change in `settings_editor_tour`. Each changed field shows **Modified** and **Reset**. |
| Open **Sort**, move with Up or Down, and press Enter. Reopen the menu and press Escape. | Enter saves and closes the choice. Escape closes the menu, leaves Settings open, and returns focus to where it was when the menu opened. |
| Enter **0** for **Page size** and press Enter. | A message below the input asks for a number of at least 1. The saved value stays 25. |
| Click **Reset** on **Page size**. | The value returns to 10 and `page_size` is removed from `settings.json`. The other edits remain. |
| On a fresh configuration, click **Edit in settings.json** for **Advanced**. Later, add `"advanced": {"flag": true}` under `settings_editor_tour` and change the greeting in the file, then save. | YARD creates `{}` if the file is missing and opens it in its associated app. Within a second the dialog reflects the saved greeting and marks **Advanced** modified. |
| Inspect `settings.schema.json` beside `settings.json`. | It declares the tour fields, their help, the sort choices and the page-size range, as well as the built-in **Theme** and **Network** sections. |
| Add `"$schema": "./settings.schema.json"` as a top-level property, then open `settings.json` in VS Code. Request completion inside `settings_editor_tour` and then on a value for `sort`. | VS Code suggests the field names and the **name** and **date** values. A later edit in YARD preserves the schema property. |
| Select **Theme** and set **Mode** to `dark` or `light`. Set **Ui font size**, then clear it and press Enter. Choose a **Ui density**, then choose **Use default** from the end of the list. Open **Network** and inspect **Proxy**. | YARD saves the new mode and applies the theme. Clearing the font size and choosing **Use default** remove the respective overrides. The optional proxy uses a text input. |
