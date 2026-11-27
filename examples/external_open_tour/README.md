# External open tour

Run from the repository root. Use a fresh settings directory so saved layouts and extension settings do not affect the tour.

```sh
export YARD_HOME="$(mktemp -d)"
YARD_LIFECYCLE=info,yard_shell=debug,external_open_tour=debug EXTERNAL_OPEN_TOUR=1 cargo run dev
```

In PowerShell, use these commands.

```powershell
$env:YARD_HOME = Join-Path $env:TEMP ("yard-external-open-" + [guid]::NewGuid())
$env:YARD_LIFECYCLE = "info,yard_shell=debug,external_open_tour=debug"
$env:EXTERNAL_OPEN_TOUR = "1"
cargo run dev
```

Each launch creates a new samples folder named `yard-external-open-tour-<pid>-<n>` under the system temporary directory. The tour starts at `n=0` for its process ID, skips names that already exist, and leaves all folders in place. Scroll the card to reach every button. With the variable unset, `cargo run dev` shows the ordinary dev window.

| Button | Command | What to look for |
| --- | --- | --- |
| **Open HTTPS** | `open_url("https://example.com/")` | `opened`. The default browser shows Example Domain. |
| **Open mailto** | `open_url("mailto:yard-test@example.com?subject=YARD")` | `opened`. The default mail client opens a draft to `yard-test@example.com` with subject `YARD`. |
| **Reveal sample.txt** | `reveal_path("sample.txt")` | `opened`. The file manager shows the samples directory with `sample.txt` selected. On Linux, GPUI's fallback can show the directory without selecting the file. |
| **Reveal folder** | `reveal_path("folder/")` | `opened`. The file manager shows `folder` selected in its parent. On Linux, GPUI's fallback can open the folder itself. |
| **Open sample.txt** | `open_path("sample.txt")` | `opened`. The default text editor displays the sample. |
| **Open file URL** | `open_url("file:///etc/hosts")` | `scheme not allowed`. Nothing opens. |
| **Open JavaScript URL** | `open_url("javascript:alert(1)")` | `scheme not allowed`. Nothing opens. |
| **Open product URL** | `open_url("<product scheme>://external_open_tour/loop")` | `scheme not allowed`. Nothing opens or loops back into the app. The tour uses the running product's configured scheme. |
| **Open HTTPS with credentials** | `open_url("https://user:pass@example.com/")` | `invalid URL`. Nothing opens. |
| **Open probe** | `open_path("probe.sh")` on Unix or `open_path("probe.bat")` on Windows | `executable file refused`. Nothing opens or runs. |
| **Open link.txt** | `open_path("link.txt")` | `executable file refused`. If the system prevented symlink creation, the result is `path not found`. Nothing opens or runs. |
| **Open folder** | `open_path("folder/")` | `not a file`. Nothing opens. |
| **Open missing.txt** | `open_path("missing.txt")` | `path not found`. Nothing opens. |

The path commands use absolute paths under the samples directory. `opened` means the system received the request. Check the desktop for what its handler did. On Windows, GPUI may open the parent folder without selecting the item when Explorer cannot select it.

Read `logs/lifecycle.jsonl` under `YARD_HOME` before restarting. Look for `external-open-tour-init` with the DEBUG steps `created tour samples` and `placed the external open tour card`. The `link_created` field shows whether the symlink case was available.

Each button produces an INFO `external-open` group with `extension=external_open_tour` and `kind=url`, `kind=reveal`, or `kind=open`. URL groups include `scheme`. Each hand-off logs `handed off` at DEBUG. For HTTPS, look for `host=example.com` on that line. Each refusal logs `refused` at INFO with the same `reason` as the card. In these groups, check that there are no raw paths, file names, URL paths, queries, fragments, credentials, or mail addresses.
