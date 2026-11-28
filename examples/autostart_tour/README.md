# Autostart tour

The tour is enabled by `AUTOSTART_TOUR=1`. It reads the application's login setting on startup and shows it with **Enable**, **Disable**, and **Refresh**. The setting belongs to the product identifier in the build configuration. Use a separate review product when checking a bundle.

| Action | Expected result |
| --- | --- |
| Click **Enable** twice | **Enabled** on both clicks when the system allows the change. |
| Change the setting in the system, then click **Refresh** | The status reflects the system's current setting. |
| Click **Disable** twice | **Disabled** on both clicks. |
| Enable from **Disabled**, then quit | The tour disables the entry. Relaunch and check that the status is **Disabled**. |
| Start with an existing enabled entry and quit without changing it | The entry stays enabled. |

The lifecycle trace shows the `autostart-tour-init`, `autostart-status`, and `autostart-set` groups.

## macOS

Run `AUTOSTART_TOUR=1 cargo run dev` to check the unbundled case. The card reads **NotSupported** and changing the setting is refused.

On macOS 13 or later, build a signed review bundle from the repository root. Use a product TOML with a different name, identifier, and URL scheme from `app/yard/product.toml`.

```sh
cargo xtask bundle macos --dev --config <review-product.toml> --env AUTOSTART_TOUR=1
```

Launch the resulting `.app` from `dist/` or `~/Applications`. After enabling, look for the application under **System Settings → General → Login Items → Open at Login** and for its identifier in `sfltool dumpbtm`. On macOS 26, select the item and click **−** to remove it, then click **Refresh** in the tour to see **Disabled**. Enable it again, then click **Disable** in the tour and verify that the item disappears from Login Items.

The tour shows **NeedsApproval** when macOS requires approval. This can happen on a managed Mac or when a background item is turned off under **App Background Activity**. This state may be unavailable on some Macs. If you see it, restore approval in System Settings.

It is normal for the probe bundle to leave a disabled record that appears only in `sfltool dumpbtm`.

## Windows

In PowerShell, run the tour from the repository root.

```powershell
$env:AUTOSTART_TOUR = '1'
cargo run dev
```

After enabling, query the `Run` value. Use the product identifier from your build configuration in place of the sample identifier if it differs.

```powershell
reg query 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v vn.risotech.yard
```

The data is the quoted executable path. Disable the application in **Task Manager → Startup apps**, then click **Refresh** and **Enable**. Both show **NeedsApproval**, and Task Manager still shows the app as disabled. Restore approval there, then use the card's **Disable** and verify that `reg query` reports a missing value.

To check uninstall, build and install a review bundle with `cargo xtask bundle windows --dev --config <review-product.toml>`. Verify that installation alone leaves the value absent. From the PowerShell session with `AUTOSTART_TOUR` set, run the installed executable with the `dev` argument. Enable through the card, then end the process from Task Manager to preserve the entry for the uninstall check. Uninstall and verify that the review product's value is gone.

## Linux

Use an isolated XDG config directory for the tour. Run from the repository root.

```sh
export XDG_CONFIG_HOME="$(mktemp -d)"
AUTOSTART_TOUR=1 cargo run dev
```

After enabling, inspect and validate the desktop entry. Use the product identifier from your build configuration in place of the sample identifier if it differs.

```sh
desktop-file-validate "$XDG_CONFIG_HOME/autostart/vn.risotech.yard.desktop"
```

The entry contains the product name, the quoted executable path, and `X-GNOME-Autostart-enabled=true`. Add `Hidden=true` in the `[Desktop Entry]` section and click **Refresh** to see **Disabled**. Remove that line, change `X-GNOME-Autostart-enabled` to `false`, and click **Refresh** again. Enable from the card to restore the entry, then disable and check that the file is gone. In a Linux review bundle, the entry points to the inner executable and starts it without the wrapper's `dev` argument.
