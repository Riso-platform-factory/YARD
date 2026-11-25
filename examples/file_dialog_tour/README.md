# File dialog tour

| Command | What to look for |
| --- | --- |
| `FILE_DIALOG_TOUR=open cargo run dev` | Click **Open file**. Choose one file. Its path appears below the button. |
| `FILE_DIALOG_TOUR=open-many cargo run dev` | Click **Open files**. Select several files. Each path appears on its own line. |
| `FILE_DIALOG_TOUR=folders cargo run dev` | Click **Open folders**. Select several folders. Each path appears on its own line. |
| `FILE_DIALOG_TOUR=mixed cargo run dev` | Click **Open files and folders**. On macOS, select a file and a folder. On Linux and Windows, select folders. Look for the folder chooser warning on Linux and Windows. |
| `FILE_DIALOG_TOUR=save cargo run dev` | Click **Choose save path**. The chooser starts in your home directory with `Untitled.txt` suggested. Confirm a name and look for its path below the button. The tour only returns the path. |

The `file-dialog` lifecycle group includes `extension=file_dialog_tour` and
`kind=open` or `kind=save`. Open cases also show the `files`, `directories`,
and `multiple` flags. Look for the debug line `shown`, followed by `chosen`
with the selected paths and their count. The save case reports a count of one.

Cancel any chooser to see `cancelled` below the button and at info level in the
trace. When the system cannot show a chooser, the card also says `cancelled`
because both outcomes deliver `None`. The reason appears at error level in
the trace and in the diagnostic log.

Without `FILE_DIALOG_TOUR`, the extension contributes nothing.
