# Connect Firefox (cookies)

Some sites only serve content to logged-in users: Instagram, Facebook, private or age-restricted videos. Sometimes YouTube also shows "Sign in to confirm you're not a bot". yt-dlp can reuse the login session of a browser installed on the same computer by reading its cookies (`--cookies-from-browser`).

## How it works in dropdl

1. Log in to the site in Firefox.
2. Click **Connect Firefox**, then **Connect**. dropdl runs `yt-dlp --cookies-from-browser firefox --list-impersonate-targets`. This loads the cookies **locally, without network access**, and reports how many were found. The command and yt-dlp's messages (profile, cookie count, errors; never cookie values) are written to the Log.
3. If it succeeds, Firefox is remembered and `--cookies-from-browser firefox` is added to every download. The button then shows "🍪 Firefox".
4. **Disconnect** removes it.

dropdl never copies, stores or uploads cookies itself. yt-dlp reads them from Firefox's profile at download time.

## Why only Firefox

Firefox stores cookies in a plain SQLite file that yt-dlp reads the same way on every platform. Chromium browsers (Chrome, Edge, Brave, Opera, Vivaldi) don't work reliably (checked October 2026):

- **Windows:** since Chrome 127, cookies use *app-bound encryption*: the key is tied to the browser itself, so other programs can't decrypt them (yt-dlp issue #10927). Chromium browsers also lock the cookie file while they run (#7271).
- **macOS:** the key is in the Keychain. yt-dlp can read it, but macOS asks for permission first. Safari needs Full Disk Access. On macOS 27 every browser's data folder is blocked anyway (see below).

Versions before October 2026 offered the Chromium browsers. A saved connection to one of them is removed on startup.

## macOS: Full Disk Access

macOS 27 protects the data folders of browsers and other apps that hold sessions (`~/Library/Application Support/Firefox`, `Google/Chrome`, `Discord`, …) from every other program: listing them fails with "Operation not permitted". yt-dlp then only reports `could not find firefox cookies database in '…/Firefox/Profiles'`.

dropdl checks the folder itself when the cookie test or a download with Firefox connected fails. If macOS denies access, it shows "macOS blocks access to Firefox's data. Give dropdl Full Disk Access, then restart dropdl." with an **Open Privacy Settings** button, and writes the OS error to the Log.

To allow it:
1. System Settings > Privacy & Security > Full Disk Access (or **Open Privacy Settings** in dropdl).
2. Click **+** and add `dropdl.app`. When dropdl runs from a terminal (`cargo run`), add the terminal app instead: macOS checks the app that started the process.
3. Quit dropdl (⌘Q) and open it again.

An app can't ask for Full Disk Access with a dialog, and no `Info.plist` key requests it; the user has to add the app in System Settings. macOS ties the permission to the app's code signature. dropdl is signed ad hoc, so every new version of `dropdl.app` needs the permission again: remove dropdl from the list, add it again and restart it (see platforms.md).

## Troubleshooting

| Problem | What to try |
|---|---|
| "could not find firefox cookies database" | Firefox isn't installed or has never been opened, so it has no profile. On macOS, see Full Disk Access above. |
| "macOS blocks access to Firefox's data" | Give dropdl Full Disk Access (above). |
| Still "login required" | Make sure you're logged in to that site in Firefox, then click **Test again**. |
