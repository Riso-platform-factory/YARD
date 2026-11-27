# Close guard tour

From the repository root, run `YARD_HOME=/tmp/yard-close CLOSE_TOUR=on cargo run dev`. Each new window starts with a card showing **Edit**, **Save**, **New window**, and `ready`. Run `cargo run dev` without `CLOSE_TOUR` for the ordinary dev window.

Read `/tmp/yard-close/logs/lifecycle.jsonl` before restarting. To include DEBUG steps from the tour, set `YARD_LIFECYCLE=info,close_tour=debug,yard_shell=debug,yard_ui=debug`.

On Windows, set `$env:YARD_HOME = "$env:TEMP\yard-close"` and `$env:CLOSE_TOUR = "on"` in PowerShell before running `cargo run dev`. Use that home for the paths below. On Linux and Windows, use **Ctrl+Q** for Quit and the title bar's close button for the close cases.

## Edit and close

- Click **Edit**. The card reads `edited`, and on macOS a dot appears in the red close button. Click **Save** on the card. It reads `saved` and the dot clears. On Linux and Windows, check the card for the edited state.
- Try each answer with the red close button. Start each case with an edited window. **Save** waits one second, clears the edited state, and closes the window. **Don't Save** closes immediately. **Cancel** keeps the window open and edited.
- Close an unedited window. It closes without a prompt.
- With just one edited window open, close it and choose **Save** or **Don't Save**. The app quits after that window closes. The trace shows one guard pass with `intent=CloseWindow`.

## Quit two windows

Use **New window** and click **Edit** in both windows. Focus one window and press **Cmd+Q**. The focused window prompts first. Choose **Don't Save** in that window and **Cancel** in the other. Both windows stay open. Repeat from the application menu's **Quit**, this time choosing **Save** in the first window and **Cancel** in the second. Both windows stay open and the first card shows `saved`. In a final run, allow both windows and verify that the app quits.

## Always cancel and turn the extension off

- Run `YARD_HOME=/tmp/yard-close CLOSE_TOUR=cancel cargo run dev`. The close button, **Cmd+Q**, and the application menu's **Quit** keep the window open. The `close-tour-guard` group records **always cancelling** at INFO.
- Add `"extensions": {"close_tour": false}` to `/tmp/yard-close/config/settings.json`. Preserve any existing settings. Start again with `CLOSE_TOUR=on`. The card is absent, the startup trace reports that the extension is turned off, and the close button closes the window without a guard. Remove the setting to restore the tour.

## Dock Quit on macOS

Start with `CLOSE_TOUR=on` and click **Edit**. Choose **Quit** from the app's Dock icon. GPUI ends the app without a prompt. See [YARD issue #17](https://github.com/Riso-platform-factory/YARD/issues/17) for the session-end limit.
