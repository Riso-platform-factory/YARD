# secrets: keeping a password or token in the system's credential store

An extension that signs in somewhere has a token or password to keep. It must not go in `settings.json`, the extension store, or the data file. Each of those is plain text or plain SQLite under the user's home. The operating system has a store for exactly this: the Keychain on macOS, the Secret Service on Linux, and Credential Manager on Windows.

This slice lets an extension keep, read, and delete a small secret there, through its handle. Each entry is named after the application and the extension, so two applications made from YARD, or two extensions, never see each other's.

The design follows GPUI's `write_credentials`, `read_credentials`, and `delete_credentials`, and Zed's `credentials_provider`, at Zed rev `ddb90e5`. Zed's provider is GPL and is read only. YARD does not call GPUI's methods, because they:

- hardcode Zed's names (the Linux label `zed-github-account`, the Windows prefix `zed:url=`);
- return opaque errors;
- copy the secret through buffers that are never wiped.

Instead, YARD calls the same system APIs directly, with the crates GPUI uses, all already in `Cargo.lock` under permissive licences:

- `objc2-security` on macOS;
- Zed's fork of `oo7` on Linux;
- the `windows` crate's `Win32_Security_Credentials` on Windows;
- `zeroize`, to wipe a secret from memory.

Electron's `safeStorage`, Tauri's Stronghold and keyring plugin, and Qt Keychain were read for comparison. The sources are in the proposal.

## What an extension writes

```rust
let save = ext.set_secret("api-token", token.as_bytes(), cx);
cx.spawn(async move |_| save.await).detach();

let read = ext.get_secret("api-token", cx);
cx.spawn(async move |_| {
    match read.await {
        Ok(Some(secret)) => use_token(secret.expose_str()?),
        Ok(None) => ask_to_sign_in(),
        Err(error) => show_error(error.to_string()), // "secret store unavailable", never the key
    }
    anyhow::Ok(())
}).detach();

ext.delete_secret("api-token", cx).detach(); // a missing entry is not an error; a dropped task is cancelled
```

- **`Extension::set_secret(key, value: &[u8], cx: &App) -> Task<Result<(), SecretError>>`** stores or replaces the entry.
- **`Extension::get_secret(key, cx: &App) -> Task<Result<Option<Secret>, SecretError>>`**. `None` means there is no entry.
- **`Extension::delete_secret(key, cx: &App) -> Task<Result<(), SecretError>>`**. Deleting a missing entry is `Ok(())` on every system.
- **The system call runs off the UI thread**, on the background executor, as GPUI's does, since a system prompt can wait for the user.
- **`Secret`** holds the bytes in a `zeroize::Zeroizing<Vec<u8>>`, wiped when dropped.
  - `expose() -> &[u8]` and `expose_str() -> Result<&str, Utf8Error>` read them.
  - Its `Debug` prints `Secret(<redacted>)`. It has no `Display`.
  - The value given to `set_secret` is copied into a zeroizing buffer before the call leaves the UI thread.
- **`SecretError`** (`Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Display`, `Error`), with fixed texts that never name the key, the value, or the extension:
  - `InvalidKey` ("invalid secret key");
  - `TooLarge` ("secret too large");
  - `Unavailable` ("secret store unavailable"): no store is running or reachable, such as Linux without a Secret Service;
  - `Denied` ("access to the secret store denied"): the user cancelled a prompt, or the store is locked and was not unlocked;
  - `Failed` ("secret store failed"): any other failure of the system call.

## The rules

- **A key** is 1 to 128 bytes of `a-z 0-9 . _ -`, lowercase only. Anything else is `InvalidKey`, checked before any system call. Windows compares credential names ignoring case, so two keys that differ only in case would be one entry there; refusing capitals makes every system behave the same.
- **A value** is at most `MAX_SECRET_BYTES` = 2560 bytes, the limit of Windows Credential Manager, on every system so an extension behaves the same everywhere (`TooLarge`). An empty value is allowed. A larger secret is stored as a key here and the ciphertext elsewhere.
- **The names**, from the product's identifier (for example `vn.example.notes`), the extension's name, and the key:
  - macOS: a generic password (`kSecClassGenericPassword`) with service `<identifier>` and account `<extension>/<key>`. It is updated in place when it exists, so the user's "Always Allow" survives.
  - Linux: an item with the label `<product name>: <extension>/<key>` and the attributes `application = <identifier>`, `extension = <extension>`, `key = <key>`, found by its attributes.
  - Windows: a generic credential with target `<identifier>/<extension>/<key>` and user name `<extension>`, persisted for the local machine.
- **An extension reaches only its own entries.** There is no listing. Extensions are native code compiled into the application, trusted as the application is: the separation keeps them from colliding by mistake, not from a hostile extension, which could call the system store itself.
- **Before the store is set up** (`yard_shell::set_secret_store`), every call returns `Unavailable` with one `ERROR` per application, as `yard_shell/data` does before its file is set. The `ERROR` is logged when the first such request runs on the UI thread, so a caller that drops that task before it runs does not see it logged.
- **`SecretStore::system` checks its names:**
  - an identifier that is empty, longer than 1024 bytes, or holds a control character or `/` gives a store whose every call returns `Unavailable`, with one `ERROR`;
  - so does a product name that is empty or holds a control character. A product name is only shown (the Linux label), so `/` is allowed in it.

  The build config already refuses such an identifier.
- **On Linux:**
  - a read or delete looks up the default collection without creating it, and a missing collection means no entry;
  - a collection still locked after the unlock request is `Denied`;
  - D-Bus errors are mapped by name:
    - `AccessDenied` is `Denied`;
    - a missing service, name, server, or bus connection (`ServiceUnknown`, `NameHasNoOwner`, `NoServer`, `Spawn.*`, a disconnected or unreachable bus) is `Unavailable`;
    - anything else, a timed-out call included, is `Failed`.
- **On macOS**, `errSecNoAccessForItem` is `Denied` too.

## Setup and tests

- **`yard_shell::set_secret_store(cx, SecretStore::system(identifier, product_name))`** is called once at startup, next to `set_data_file`. A second call is refused with an `ERROR`.
- **`SecretStore::memory()`** is an in-memory store for tests. It holds what is set, per extension and key, honours every rule above, and can be told to fail the next call with a chosen `SecretError` (`fail_next`). `SecretStore` is `Clone`, and clones share one store, so a test keeps a handle after giving one to `set_secret_store`.
- **While the part is under review**, `review.rs` sets the system store. **At promotion:**
  - `yard_app/startup` sets the system store from the product;
  - `yard_test/harness` sets a memory store in `TestApp::boot`, so an extension's tests never touch the real store. It does so through `yard_app::init_with_secret_store`, which is `init` with a store given in place of the system store, set before any extension starts.

  Both are production parts, shown and stamped again then.

## Logging

- **`secret{extension, op}`** surrounds each call, the checks and the system call. `op` is `set`, `get`, or `delete`. The group is carried onto the background task with `.instrument`.
- **It never logs the key, the value, or the entry's name.** The key can carry personal data, such as a user's email in a token name.
- `debug!("stored")`, `debug!(found, "read")`, and `debug!("deleted")` on success.
- **A refusal** (`InvalidKey`, `TooLarge`): `info!(reason = %error, "refused")`.
- **`Denied`:** `info!(reason = %error, "denied")`, since the user chose it.
- **`Unavailable` and `Failed`:**
  - `warn!` in the group, and one `log::warn!` with the fixed text and the system's numeric status where there is one, never the key or names;
  - not `ERROR`, since the application goes on and the extension decides what to tell the user.

## Parts

- **`yard_shell/secrets`** (`crates/yard_shell/src/dev/secrets.rs`, declared in `src/dev.rs`):
  - the three methods, `Secret`, `SecretError`, `MAX_SECRET_BYTES`;
  - `SecretStore` with its system and memory stores;
  - `set_secret_store`, the rules, and the logging.

  The system store's code for each OS sits in `#[cfg(target_os = …)]` sections of the part. The part adds to `yard_shell`'s dependencies, as workspace dependencies at the versions already locked:
  - `zeroize`;
  - on macOS, `objc2-security` and `objc2-core-foundation`;
  - on Linux, Zed's `oo7` (same git revision, `default-features = false`, the features GPUI uses);
  - on Windows, the `windows` feature `Win32_Security_Credentials`.
- **Acceptance tests**, written first by a worker that does not write the code, against `SecretStore::memory()`:
  - set, get, overwrite, and delete;
  - get and delete of a missing entry;
  - two extensions do not see each other's entries;
  - the system store's entry names, built by a private `entry_names(identifier, product_name, extension, key)` and tested in the part's own `tests.rs`, differ for two identifiers and for two extensions, and match the names above on each system;
  - every refusal, checked before the store is reached;
  - each injected failure maps to its variant;
  - `Debug` of a `Secret` and every error text hold neither key nor value;
  - calls before `set_secret_store` return `Unavailable` with one `ERROR`;
  - in their own file, run 20 times: the group, its fields, and the levels above, and that neither key nor value appears in either log.

  The system stores are tested by the test extension, and by small tests of the name building.
- **Test extension `examples/secrets_tour`**, named in `review.rs`.
  - Without `SECRETS_TOUR` it does nothing.
  - With `SECRETS_TOUR=1`, it places a card with buttons, each showing its result:
    - set `tour-key` to a fixed test value;
    - get it: shows its length and the value;
    - overwrite it with another value, and get it again;
    - delete it, and get it again: shows "no entry";
    - delete it again: still `Ok`;
    - set a 2561-byte value: `TooLarge`;
    - set the key `bad key!`: `InvalidKey`.
  - The tour deletes `tour-key` when it starts, and at quit if it wrote it. GPUI gives quit work 200 ms, so a slow store can leave the entry until the next start, which deletes it.
  - The README names, for each system, where to see the entry: Keychain Access, Seahorse, or `cmdkey /list`.

## Known limits

- **macOS asks again after a rebuild.** The Keychain ties "Always Allow" to the binary's signature, so an unsigned or ad-hoc-signed development build prompts each time it is rebuilt. A build signed with a stable identity does not. There is no plain-text fallback for development, unlike Zed's.
- **Linux without a Secret Service** (a headless session, a minimal window manager, most CI runners) returns `Unavailable`. A locked keyring asks the user to unlock it, and dismissing that is `Denied`.
- **Windows Credential Manager does not separate applications.** Any program running as the same user can read a generic credential. This is the system's model, as for every application that uses it.
- **2560 bytes everywhere**, though macOS and Linux would take more.
- **Copies held by the system libraries are freed without being wiped:** the `CFData` the Keychain returns on macOS, `oo7`'s decryption buffer on Linux, and the message bodies of a plain (unencrypted) Secret Service session, which `oo7` falls back to when the service refuses an encrypted one. YARD wipes every copy it owns.
- **Linux sandboxes (Flatpak, Snap) are not supported:** the store talks to the Secret Service over D-Bus, not through the desktop portal.
- **Two product identifiers that differ only in case** share their entries on Windows, which compares credential names ignoring case. Identifiers are reverse-DNS names, normally lowercase.

## Not in this slice

- The macOS Data Protection keychain, access groups, Touch ID, and Windows Hello policies. They need an Apple developer account and entitlements, which this slice does not ask for.
- Listing an extension's entries, and sharing entries between extensions or applications.
- An encrypted vault file of YARD's own, as Tauri Stronghold has.
- Secrets larger than 2560 bytes.
