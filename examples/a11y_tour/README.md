# Accessibility tour

Run `A11Y_TOUR=1 cargo run dev` from the repository root. In Windows PowerShell, run `$env:A11Y_TOUR = '1'` followed by `cargo run dev`. Run without `A11Y_TOUR` to verify the tour card and its status bar item are absent.

| Action | What to check |
| --- | --- |
| Inspect the window | The root carries the application title. Open another window and check its title too. |
| Activate **Select next choice** | It is a button named “Select next choice”. The list selection advances. |
| Activate the icon button | It is a button named “Refresh”. The first tab and first choice become selected. Refresh also reopens the Details tab after closing it. |
| Focus **Name** and type | It is a text input named “Name”, with placeholder “Your name”. The value reflects the text. If the inspector supports setting the value, replace it and type one character to check that the caret is at the end. |
| Move through the tabs | The **Overview** and **Details** tabs are in one tab list. Inspect their selection. On Windows and Linux, check their positions (1 and 2) and count (2). The close button is named “Close Details”. |
| Move through the list | The list has three list box options named “First choice”, “Second choice”, and “Third choice”. The first starts selected. Activate another row and check that the selection moves. |
| Activate **Open About** | A dialog named “About” receives focus. Its **Close** button closes it. Use **Dump tree** inside the dialog to capture it while open. |
| Activate **Show info toast** | Listen for “Information from the tour”. Inspect the toast’s Status role and name. Its **Dismiss** button is named “Dismiss”. |
| Activate **Show error toast** | Listen for “Error from the tour”. Inspect the toast’s Alert role and name. Its **Dismiss** button is named “Dismiss”. |
| Open the command palette with Cmd+Shift+P on macOS or Ctrl+Shift+P on Windows or Linux, then move the highlight | It is a dialog named “Command palette” with an input named “Command”. Each result is a list box option named for its command. Inspect the selected state on the highlighted row. |
| Inspect the drawn caption buttons on Windows or Linux | Their names are “Minimize”, “Maximize”, and “Close”. Maximize the window and check that the middle button becomes “Restore”. |
| Inspect the status bar showing **Accessibility tour** | Its role is Status. |
| Activate **Dump tree** | With assistive technology active, the button writes `a11y-tree.json` in the logs directory. Try it before starting an inspector or screen reader to check for the INFO event `refused` with reason `no assistive technology is active` in `a11y-tour-dump`. |

Set `YARD_HOME` to an empty review directory to find the dump and logs under its `logs/` directory. The dump contains exposed values, including the current **Name** text. Use sample text for the tour. Dump each toast before it times out, then save each file under a distinct name if you need both snapshots.

## macOS

Turn on VoiceOver with Cmd-F5 and navigate to the YARD window and its controls. Open **Accessibility Inspector** from Xcode’s developer tools and inspect the window hierarchy, names, and states.

## Windows

Start **Narrator** with Win+Ctrl+Enter and navigate through the YARD window. Use **Inspect.exe** from the Windows SDK in UI Automation mode to inspect the control tree and properties.

## Linux

Start the **Orca** screen reader in your desktop session and navigate through the YARD window. Open **Accerciser**, select YARD in the application tree, and inspect its nodes and states over AT-SPI.
