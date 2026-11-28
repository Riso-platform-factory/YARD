# Tray tour

From the repository root, run `TRAY_TOUR=1 cargo run dev`. In PowerShell, run `$env:TRAY_TOUR = '1'; cargo run dev`. The tour does nothing unless `TRAY_TOUR` is set to `1`.

| Action | Expected result |
| --- | --- |
| Click **Show tray** | A diamond appears in the system tray. Hover to see **YARD tray tour** when the desktop supports tooltips. |
| Choose **Say hello**, then **More → Inner** | Each selection increments its counter and updates the result line. Try with another app focused as well. |
| Click **Change menu** | The tooltip becomes **YARD tray tour updated**. Select **Again** and look for it on the result line. |
| Choose **New Window** | YARD opens another window. |
| Click **Invalid icon** while the tray is showing | The result displays **InvalidIcon · tray icon is not a valid PNG** and the existing tray stays usable. |
| Click **Remove tray**, then **Show tray** | The icon disappears, then returns with the original menu. |
| Choose **Quit** while the icon is visible | The app quits and removes the icon. |

## System checks

- **macOS** · Try both light and dark menu bars. The diamond uses the system's template colour at 18 points high. Clicking opens the menu.
- **Windows** · Left-click to increment **Activations** and right-click to open the menu. Restart Windows Explorer while the icon is showing and check that the icon and menu return. Check the icon at different display scaling settings.
- **Linux** · On KDE and on GNOME with the AppIndicator extension enabled, open the tray context menu and exercise the actions. A primary activation request increments **Activations**. On GNOME without AppIndicator, **Show tray** reports **Unavailable · no tray on this desktop**.

The lifecycle trace includes `tray-tour-init` when the tour is enabled, along with the tray groups for each attempted operation.
