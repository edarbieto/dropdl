# Connect Firefox (cookies)

Some sites only serve content to logged-in users: Instagram, Facebook, private or age-restricted videos. Sometimes YouTube also shows "Sign in to confirm you're not a bot". yt-dlp can reuse the login session of a browser installed on the same computer by reading its cookies (`--cookies-from-browser`).

## How it works in dropdl

1. Log in to the site in Firefox.
2. Click **Connect Firefox**, then **Connect**. dropdl runs `yt-dlp --cookies-from-browser firefox --list-impersonate-targets`. This loads the cookies **locally, without network access**, and reports how many were found.
3. If it succeeds, Firefox is remembered and `--cookies-from-browser firefox` is added to every download. The button then shows "🍪 Firefox".
4. **Disconnect** removes it.

dropdl never copies, stores or uploads cookies itself. yt-dlp reads them from Firefox's profile at download time.

## Why only Firefox

Firefox stores cookies in a plain SQLite file that yt-dlp reads the same way on every platform. Chromium browsers (Chrome, Edge, Brave, Opera, Vivaldi) don't work reliably (checked October 2026):

- **Windows:** since Chrome 127, cookies use *app-bound encryption*: the key is tied to the browser itself, so other programs can't decrypt them (yt-dlp issue #10927). Chromium browsers also lock the cookie file while they run (#7271).
- **macOS:** the key is in the Keychain. yt-dlp can read it, but macOS asks for permission first. Safari needs Full Disk Access.

Versions before October 2026 offered the Chromium browsers. A saved connection to one of them is removed on startup.

## Troubleshooting

| Problem | What to try |
|---|---|
| "could not find firefox cookies database" | Firefox isn't installed or has never been opened, so it has no profile. |
| Still "login required" | Make sure you're logged in to that site in Firefox, then click **Test again**. |
