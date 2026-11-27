# external-open: opening a link, revealing a file, opening a file

An application made from YARD often needs to hand something to the operating system: a help link opened in the browser, a `mailto:` link in the mail client, a saved file shown in Finder or Explorer, a document opened in the application that owns its type. GPUI can do all three, but it checks nothing, returns nothing, and on some failures logs the full URL or path.

This slice gives every extension three methods on its handle. Each checks what it is given before GPUI sees it, refuses what could run a program or reach the network unexpectedly, and logs without the user's data.

The design follows GPUI's `App::open_url`, `App::reveal_path`, and `App::open_with_system`, and Zed's callers of them (`OpenBrowser`, `reveal_in_finder`, `open_system`), at Zed rev `ddb90e5`. The checks follow Electron's security guideline 15 (`shell.openExternal` with untrusted content) and Tauri's opener plugin, whose default scope allows `http`, `https`, and `mailto`. The sources are in the proposal.

## What GPUI gives

- `open_url(&self, url: &str)`, `reveal_path(&self, path: &Path)`, `open_with_system(&self, path: &Path)`, on `&App`. All three return `()`.
  - On macOS, `open_url` calls `NSWorkspace openURL:` on the UI thread. The other two, and all three on Linux and Windows, run on a background thread.
  - Linux uses `xdg-open` and its fallbacks first, then the XDG desktop portal. Windows uses `ShellExecuteW` and `SHOpenFolderAndSelectItems`. macOS uses `NSWorkspace` and `open --`.
- **None of them checks anything.** On Windows, `ShellExecuteW` runs a program given by path or by name, opens a UNC path, and follows any registered scheme (`ms-msdt:`, `search-ms:`, `file:`). macOS `open` runs an `.app` bundle or a script. Zed passes every scheme through.
- **On failure, GPUI logs the raw value.** On macOS, a string `NSURL` refuses is logged. On Windows, a failed `ShellExecuteW` is logged with the full URL or path.
- GPUI's test platform records the last `open_url` (`TestAppContext::opened_url`). Its `reveal_path` and `open_with_system` panic (`unimplemented!()`).

## What an extension writes

```rust
if let Err(error) = ext.open_url("https://example.com/help", cx) {
    show_error(error.to_string()); // "scheme not allowed", never the URL
}
ext.reveal_path(&exported_file, cx)?;
ext.open_path(&exported_file, cx)?;
```

- **`Extension::open_url(url, cx)`**, **`Extension::reveal_path(path, cx)`**, **`Extension::open_path(path, cx)`**, with `cx: &App`. Each returns `Result<(), ExternalOpenError>`.
- **`Ok(())` means the request passed the checks and was given to GPUI.** What the operating system does afterwards cannot be reported: GPUI does not return it, and most of the work runs on another thread.
- **A refusal returns before GPUI is called.**
- **`ExternalOpenError`** (`Clone`, `Debug`, `PartialEq`, `Eq`, `Display`, `std::error::Error`). Each variant shows a fixed text that names no URL, address, or path:
  - `InvalidUrl` ("invalid URL");
  - `UrlTooLong` ("URL too long");
  - `SchemeNotAllowed` ("scheme not allowed");
  - `InvalidPath` ("invalid path");
  - `NotFound` ("path not found");
  - `NotAFile` ("not a file");
  - `Executable` ("executable file refused").

## The checks

### `open_url`

1. **The raw text must be valid**, checked before anything parses it:
   - it is not empty, and is at most 8192 bytes (`UrlTooLong`);
   - it has no whitespace or control character (`InvalidUrl`). This includes tab, CR, and LF, which a WHATWG parser would silently drop;
   - it has none of `\ < > " { } | ^` and backtick, and every `%` starts a valid `%XX` escape;
   - it has no `[` or `]`, except around an IPv6 host right after `//`, in any scheme (so `ftp://[::1]/` is then refused for its scheme). macOS 12 and 13 refuse them elsewhere, and GPUI then logs the URL.
2. **No escape decodes to a control character** (`InvalidUrl`): `%00` to `%1F` and `%7F` are refused anywhere. The one exception is `%0D` and `%0A` in the value of a `mailto` URL's `body` field (its name spelled `body`, ignoring case), where they mean line breaks, as RFC 6068 allows.
3. **It must parse** with the `url` crate (`InvalidUrl`). The crate is already in `Cargo.lock` through Zed. The parsed URL's serialization must also be at most 8192 bytes (`UrlTooLong`), since percent-encoding and punycode can make it longer than the input.
4. **Only three schemes are allowed: `https`, `http`, and `mailto`**, compared ignoring case (`SchemeNotAllowed`). So the following are all refused:
   - `file:` and `javascript:`;
   - `data:` and `vbscript:`;
   - every OS or custom scheme;
   - the application's own scheme, which would come back to the running application.
5. **For `http` and `https`, there must be a host, and no user name or password** (`InvalidUrl`). A user name or password in the URL can be used to disguise a link's real host. The raw text must have `//` and a non-empty authority, so the parser cannot invent a host for `http:///path`.
6. **For `mailto`, every recipient must be an address** (`InvalidUrl`):
   - there is no `//`;
   - the part before `?` holds at least one recipient;
   - split on `,`, each recipient has exactly one literal `@`. Its local part and domain are each non-empty once percent-decoded, valid UTF-8, and hold no whitespace, control character, or `@`. So `a%40example.com` and `%20@%20` are refused;
   - the same rule applies to each recipient in a `to`, `cc`, or `bcc` field of the query;
   - no field name appears twice in the query, ignoring case, so a client cannot pick between two `body` or `subject` values.
7. **GPUI gets the parsed URL's serialization**, so what was checked is what opens.

### The path rules

Both path methods first apply these rules to the path they are given (`InvalidPath`):

- it is absolute, valid UTF-8 (GPUI on macOS turns any other path into an empty string), and has no control character;
- it has no `.` or `..` segment, so it names exactly the folder it spells. This is checked on the text, on every system, since Rust drops a `.` on Unix but Windows keeps it;
- it is not a filesystem root, since a root has no parent folder to show it in;
- on Windows:
  - it is a plain drive path, `C:\…`. A UNC, `\\?\` (verbatim), `\\?\UNC\`, or `\\.\` (device) path is refused;
  - its drive is local. `GetDriveTypeW` on the drive's root must give a fixed, removable, CD-ROM, or RAM drive. A remote (mapped) drive, or a drive with no root, is refused **before any file-system call**. A local link to a share can still be followed while it is resolved (see "Known limits");
  - no component ends in a dot or a space, or is a DOS device name (`CON`, `PRN`, `AUX`, `NUL`, `COM0`–`COM9`, `COM¹`–`COM³`, `LPT0`–`LPT9`, `LPT¹`–`LPT³`, `CONIN$`, `CONOUT$`, with or without an extension, ignoring case), since Windows rewrites or redirects such names;
  - no component after the drive holds `:`, which names an alternate data stream;
  - the path GPUI gets uses `\` only, never `/`, since Explorer refuses `/` and GPUI then logs the path.

The same rules apply again to the resolved path, after symbolic links are followed. On Windows, the resolved `\\?\C:\…` form is turned back into `C:\…` first. A resolved path that breaks a rule, such as one on a share, is refused (`InvalidPath`).

### `reveal_path`

Showing a file in the file manager never runs it, so it is allowed on any existing regular file or folder.

1. **The path rules**, on the given path (`InvalidPath`).
2. **It must exist**, symbolic links followed (`NotFound`). The rules are checked again on the resolved path (`InvalidPath`).
3. **It must be a regular file or a folder** once links are followed (`NotAFile`). A socket, FIFO, or device is refused: GPUI on Linux opens the path to reveal it, and a FIFO would block.
4. **GPUI gets the path as given**, so a link is shown as the link (on Windows, with `\` separators).

### `open_path`

Opening a file hands it to the application that owns its type, which for a program means running it.

1. **The path rules**, on the given path (`InvalidPath`).
2. **The path is resolved with `std::fs::canonicalize`** (`NotFound` if it fails), and the rules are checked again on the resolved path (`InvalidPath`). Every rule below applies to the resolved file, so a `notes.txt` link that points at a script is judged as the script.
3. **It must be a regular file** (`NotAFile`): a folder, including a macOS `.app` bundle, a socket, a FIFO, or a device is refused. To show a folder, an extension uses `reveal_path`.
4. **It must not be a program, a script, an installer, or a launcher** (`Executable`):
   - on Unix, any execute bit is set;
   - or its extension, on the given path or the resolved one and ignoring case, is in this list:
     - Windows: the file types Outlook blocks as attachments (Microsoft's "Blocked attachments in Outlook" list): `ade adp app application appref-ms asp aspx asx bas bat bgi cer chm cmd cnt com cpl crt csh der diagcab exe fxp gadget grp hlp hpj hta htc inf ins isp its jar jnlp js jse ksh lnk mad maf mag mam maq mar mas mat mau mav maw mcf mda mdb mde mdt mdw mdz msc msh msh1 msh2 mshxml msh1xml msh2xml msi msp mst msu ops osd pcd pif pl plg prf prg printerexport ps1 ps1xml ps2 ps2xml psc1 psc2 psd1 psdm1 psm1 pst py pyc pyo pyw pyz pyzw reg scf scr sct settingcontent-ms shb shs theme tmp url vb vbe vbp vbs vhd vhdx vsmacros vsw webpnp website ws wsb wsc wsf wsh xbap xll xnk`, and also `appx appxbundle msix msixbundle appinstaller library-ms searchconnector-ms iso img`;
     - macOS: `app command tool pkg mpkg dmg scpt scptd applescript action workflow terminal webloc inetloc fileloc ftploc mailloc prefpane`;
     - Linux and others: `desktop appimage sh bash zsh fish run bin deb rpm flatpakref rb`;
5. **GPUI gets the resolved path**, through `open_with_system`.

The checks read the file system on the UI thread: `canonicalize`, then `metadata` on the resolved path, as GPUI's own macOS `open_url` works on that thread.

## Logging

- **`external-open{extension, kind}`** surrounds each call, the checks and the hand-off to GPUI.
  - `kind` is `url`, `reveal`, or `open`.
  - For `url`, `scheme` is added when the text is at most 8192 bytes and has a syntactically valid scheme. It is lowercased, and given only when it is one of `http`, `https`, `mailto`, `file`, `javascript`, `data`, `vbscript`, `ftp`, `ms-msdt`, or `search-ms`. Any other scheme is logged as `other`, since a refused input's prefix can be a secret, as in `secret-token:…`.
  - It never logs a path, a file name, a query, a fragment, a user name, a password, or a `mailto` address.
- **`debug!(host, "handed off")`** when GPUI was called. `host` is given for an `http` or `https` URL. It is on this line, not on the group, because the lifecycle trace keeps only the fields a group has when it opens, and the host is known only after the checks.
- **`info!(reason = %error, "refused")`** for a refusal. A refusal is an expected answer, not a failure.
- Nothing goes to the diagnostic log from this part.

## Parts

- **`yard_shell/external_open`** (`crates/yard_shell/src/dev/external_open.rs`, declared in `src/dev.rs`): the three methods, `ExternalOpenError`, the checks, and the logging. It adds `url` to `yard_shell`'s dependencies, as a workspace dependency at the version already locked. It changes no production part.
- **In the application:** the methods are on every extension's handle, the place where an extension reaches the platform. The shell itself opens nothing.
- **Acceptance tests**, written first by a worker that does not write the code:
  - `crates/yard_shell/tests/external_open.rs`:
    - every accepted URL reaches `TestAppContext::opened_url` as its serialization;
    - every refused URL, and every refused path of both path methods, returns its variant, opens nothing, and does not panic;
  - `src/dev/external_open/tests.rs`: accepted paths, since GPUI's test platform panics on them. These tests call the part's checks directly: `check_url(&str) -> Result<Url, ExternalOpenError>`, `check_reveal(&Path) -> Result<PathBuf, ExternalOpenError>`, `check_open(&Path) -> Result<PathBuf, ExternalOpenError>`, all private to the part. The cases:
    - a text file;
    - a folder for `reveal_path`;
    - an executable revealed;
    - a `.txt` link to a text file opened as the resolved file;
  - `crates/yard_shell/tests/external_open_logging.rs`, alone in its file: the group, its fields, and the levels above for an accepted URL and each kind of refusal. No URL path, query, fragment, user name, password, `mailto` address, or file path appears in any line. Run 20 times.
- **Test extension `examples/external_open_tour`**, named in `review.rs`.
  - Without `EXTERNAL_OPEN_TOUR` it does nothing.
  - With `EXTERNAL_OPEN_TOUR=1`, it makes a new folder of samples in the system's temporary folder, `yard-external-open-tour-<pid>-<n>`, taking the first `<n>` whose folder it can create. It never deletes a folder:
    - `sample.txt`;
    - a script with its execute bit (`probe.sh`), or `probe.bat` on Windows;
    - `link.txt`, a symbolic link to the script, where the system allows it;
    - a folder.
  - It then places one card with a button for each case. The card shows `opened`, or the error text:
    - open `https://example.com/`;
    - open `mailto:yard-test@example.com?subject=YARD`;
    - reveal `sample.txt`;
    - reveal the folder;
    - open `sample.txt`;
    - refused, each:
      - `file:///etc/hosts`;
      - `javascript:alert(1)`;
      - the application's own scheme;
      - `https://user:pass@example.com/`;
      - open the script;
      - open `link.txt`;
      - open the folder;
      - open a missing path.

## Known limits

- **What the operating system does after the hand-off is not reported.** Examples: no default browser, a broken `xdg-open`, a file type with no application. GPUI logs some of these failures itself, and on Windows its line carries the URL or path. The checks above keep GPUI from logging on the failures they can foresee, such as an `NSURL` parse failure or a missing file.
- **On a Unix mount that gives every file an execute bit (FAT, exFAT, NTFS), `open_path` refuses every file.** `reveal_path` still works.
- **The checks and the hand-off are not atomic.** Another local process could swap the file between them. This is a guard against mistakes and hostile links, not a sandbox.
- **URLs up to 8192 bytes pass the checks.** Some older Windows handlers stop at 2083 characters.
- **The list of blocked file types is a guard, not a guarantee.** An application installed on a system can register any other type to run code when opened. The list covers the types the systems themselves run or install, and the launchers they ship.
- **On Windows, a local symbolic link or junction to a share is reached while it is resolved.** A mapped drive is refused before any file-system call, but following a local link is the only way to learn where it points. The resolved path is then refused.
- **A product whose own scheme is `http`, `https`, or `mailto` would have its links opened by the system**, not by itself. The build config does not refuse such a scheme. Real products use a scheme of their own.

## Not in this slice

- Opening a folder in the file manager. Today a folder is only revealed in its parent. `TODO(external-open-folder)`: telling a macOS bundle from a folder needs `NSWorkspace isFilePackageAtPath:`. [YARD#19](https://github.com/Riso-platform-factory/YARD/issues/19)
- Opening a file with a chosen application, as Tauri's `with` does.
- A per-extension list of allowed hosts or folders, as Tauri's scope has.
- A confirmation prompt before opening a link.
- `tel:` and other schemes an application might want. A later slice can let the application add schemes to the list.
