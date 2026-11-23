# Settings

Application and extension settings live in a single JSON file, `settings.json`, inside the application's config directory (see [Product Config](./building/product-config.md#application-directories)). Settings are organized into named top-level sections and reload automatically when the file changes.

## File Format

`settings.json` is a plain JSON object (comments and trailing commas are not supported). Each top-level key corresponds to a registered settings section, such as `theme` or an extension's name:

```json
{
  "theme": {
    "mode": "system",
    "dark": "One Dark",
    "light": "One Light",
    "ui_font_size": 16,
    "ui_density": "default"
  }
}
```

- Missing fields inside a section fall back to that section's defaults.
- Unknown top-level keys (for example, settings belonging to an extension not loaded in this build) are ignored on read and preserved when the application writes to `settings.json`.

## Validation and Fallback

- If `settings.json` does not exist, every section uses its defaults.
- If `settings.json` is unreadable, invalid JSON, or not a JSON object, YARD logs a `WARN` and uses defaults at startup (or keeps the previous valid values during a reload).
- If one section has an invalid field type or value, YARD logs a `WARN` naming the section and falls back to defaults at startup (or keeps that section's previous value during a reload). All other valid sections still apply.

## Live Reload and Programmatic Writes

YARD checks `settings.json` once every second while the application runs. When the file changes, YARD re-reads it and notifies observers only for sections whose parsed JSON value actually changed. Removing `settings.json` resets sections back to their defaults.

Changes made from inside the application (such as choosing a theme through a picker) update `settings.json` through `yard_settings::write`:

- Re-reads `settings.json` from disk before changing it, or creates the file if it does not exist.
- Leaves malformed JSON files, non-object files, and non-object sections untouched (logging a `WARN`).
- Skips writing when the field already holds the requested value.
- Preserves existing key order, other sections, and unknown keys, writing two-space pretty-printed JSON with a trailing newline atomically, then applies the change immediately.
- Validates the write against the registered section: a writable field must exist in serialized `T::default()`, and the updated section must deserialize cleanly into `T`. Calling `write` for an unregistered section, unknown field, or value that fails decoding logs an `ERROR` and writes nothing.

For declaring and writing settings sections from Rust, see [Extensions](./building/extensions.md#settings-sections).

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `settings-register` | `section` | Registering a settings section |
| `settings-load` | `path` | Loading `settings.json` at startup |
| `settings-watch` | — | Starting the `settings.json` watcher |
| `settings-reload` | `path` | Reloading `settings.json` after external change or programmatic write |
| `settings-write` | `section`, `field` | Writing one field of a settings section |
