# Connect browser (cookies)

Some sites only serve content to logged-in users: Instagram, Facebook, private or age-restricted videos. Sometimes YouTube also shows "Sign in to confirm you're not a bot". yt-dlp can reuse the login session of a browser installed on the same computer by reading its cookies (`--cookies-from-browser`).

## How it works in dropdl

1. Click **Connect browser** and pick a browser (Firefox, Chrome, Edge, Brave, Opera, Vivaldi).
2. dropdl runs `yt-dlp --cookies-from-browser <browser> --list-impersonate-targets`. This loads the cookies **locally, without network access**, and reports how many were found.
3. If it succeeds, the browser is remembered and `--cookies-from-browser <browser>` is added to every download. The button then shows the browser name.
4. **Disconnect** removes it.

dropdl never copies, stores or uploads cookies itself. yt-dlp reads them from the browser's profile at download time.

## Troubleshooting

| Problem | What to try |
|---|---|
| "could not find … cookies database" | That browser isn't installed or has no profile. Pick another. |
| Chrome/Edge/Brave: "Could not copy … cookie database" | Chromium browsers lock their cookie file while running. Close the browser completely and try again. |
| Chrome/Edge: decryption errors | Recent Chromium versions on Windows protect cookies with app-bound encryption, which yt-dlp may not be able to read. **Use Firefox** (log in to the site there once). |
| Still "login required" | Make sure you're logged in to that site in the chosen browser, then reconnect. |

Firefox is the most reliable choice.
