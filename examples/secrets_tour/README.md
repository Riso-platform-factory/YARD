# Secrets tour

From the repository root, run `SECRETS_TOUR=1 cargo run dev`. In PowerShell, use `$env:SECRETS_TOUR = '1'; cargo run dev`. With any other setting or no setting, the tour does nothing.

The buttons become available after the tour attempts to delete `tour-key`. It requests deletion at quit if it successfully wrote the entry during that run. GPUI gives quit work 200 ms, so a slow store can leave the entry for the next startup cleanup. If the system prompts for access, allow it to exercise the success cases. The card shows any startup cleanup error and lets you try the buttons.

| Action | Expected result |
| --- | --- |
| Click **Set** | The card shows **Ok**. The system credential entry appears. |
| Click **Get** | The card shows **16 bytes · first tour value**. |
| Click **Overwrite**, then **Get** | The card shows **Ok**, then **22 bytes · replacement tour value**. |
| Click **Delete**, then **Get** | The card shows **Ok**, then **no entry**. The entry disappears from the system store. |
| Click **Delete** again | The card still shows **Ok**. |
| Click **Set 2561 bytes** | The card shows **TooLarge · secret too large**. |
| Click **Set invalid key** | The card shows **InvalidKey · invalid secret key**. |
| Click **Set**, wait for **Ok**, then quit the app | The tour attempts to delete the entry during the quit window. |

The lifecycle trace shows `secrets-tour-init` and one `secret` group per operation. The read result shows the fixed test value on the card. The trace shows only whether a read found an entry.

## Find the entry in the system store

These names use the repository's default product. A custom build uses its own product name and identifier.

- **macOS** · Open **Keychain Access**, select the login keychain, and search for service `vn.risotech.yard`. Look for the generic password with account `secrets_tour/tour-key`.
- **Linux** · Open **Seahorse** and look in the default password collection for `yard: secrets_tour/tour-key`. A session without a running Secret Service reports **Unavailable**. A dismissed unlock prompt reports **Denied**.
- **Windows** · Run `cmdkey /list` and look for the generic target `vn.risotech.yard/secrets_tour/tour-key` with user `secrets_tour`.
