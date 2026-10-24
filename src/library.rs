//! Music library mode: YouTube Music songs and albums become
//! `Album Artist/Album (Year)/01 - Title.ext`, tagged by dropdl. Existing
//! tracks are skipped before downloading. Rules in docs/music-library.md.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::job::{self, AudioFormat, Event, Mode, Request, Tools};
use crate::music::{self, Album, Link, Track};
use crate::{lyrics, tags, ytm};

/// Longest file or folder name in bytes, to stay within Windows path limits.
const MAX_NAME: usize = 120;
/// Names Windows reserves for devices, also with an extension ("CON.txt").
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether a request goes through library mode: audio from any YouTube
/// Music link it understands (song, album, album playlist, artist, playlist).
pub fn handles(url: &str, mode: Mode) -> bool {
    job::wants_audio(url, mode) && music::classify(url).is_some()
}

/// Album links of an artist's releases, newest first as on the artist page.
fn release_links(artist: &ytm::Artist) -> Result<Vec<String>, String> {
    if artist.releases.is_empty() {
        return Err("No albums or singles found for this artist".into());
    }
    Ok(artist.releases.iter().map(|id| format!("https://music.youtube.com/browse/{id}")).collect())
}

/// Queues every album, single and EP of an artist as its own album job.
fn queue_artist(channel: &str, on_event: &mut impl FnMut(Event)) -> Result<(), String> {
    on_event(Event::Processing("Looking up releases".into()));
    let artist = ytm::Client::new().artist(channel)?;
    on_event(Event::Progress { frac: None, item: None, title: artist.name.clone() });
    let links = release_links(&artist)?;
    on_event(Event::Log(format!("{}: queued {} releases", artist.name, links.len())));
    on_event(Event::Enqueue(links));
    Ok(())
}

/// A file or folder name that every platform accepts: characters Windows
/// forbids become `_`, trailing dots and spaces are removed, Windows device
/// names get a `_`, and long names are shortened. Tags keep the original text.
pub fn sanitize(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    if s.len() > MAX_NAME {
        let end = (0..=MAX_NAME).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
        s.truncate(end);
    }
    let mut s = s.trim().trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() {
        s.push('_');
    }
    let stem = s.split('.').next().unwrap_or("").trim_end();
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        s.push('_');
    }
    s
}

/// `Album Artist/Album (Year)`, relative to the output folder.
pub fn album_dir(album: &Album) -> PathBuf {
    let artist = if album.artists.is_empty() { "Unknown Artist".to_string() } else { album.artists.join(", ") };
    let year = album.date.as_deref().and_then(|d| d.get(..4)).filter(|y| y.bytes().all(|b| b.is_ascii_digit()));
    let name = match year {
        Some(year) => format!("{} ({year})", album.title),
        None => album.title.clone(),
    };
    PathBuf::from(sanitize(&artist)).join(sanitize(&name))
}

/// "Album Artist – Album", the name of an album job.
fn album_label(album: &Album) -> String {
    format!("{} – {}", album.artists.join(", "), album.title)
}

/// `01 - Title.ext`, or `1-01 - Title.ext` on a multi-disc album.
pub fn file_name(album: &Album, track: &Track, ext: &str) -> String {
    let number = if album.discs > 1 {
        format!("{}-{:02}", track.disc, track.number)
    } else {
        format!("{:02}", track.number)
    };
    format!("{}.{ext}", sanitize(&format!("{number} - {}", track.title)))
}

/// The tracks a link asks for: the song itself, or the whole album.
pub fn select(album: &Album, link: &Link) -> Option<Vec<usize>> {
    match link {
        Link::Song(id) => album.tracks.iter().position(|t| t.video_id.as_deref() == Some(id)).map(|i| vec![i]),
        _ => Some((0..album.tracks.len()).collect()),
    }
}

fn extension(format: AudioFormat) -> &'static str {
    match format {
        AudioFormat::M4a => "m4a",
        AudioFormat::Mp3 => "mp3",
    }
}

/// Looks up the album of a link and the tracks to download.
fn lookup(link: &Link, log: &impl Fn(String)) -> Result<(Album, Vec<usize>), String> {
    let mut yt = ytm::Client::new();
    let ids = music::album_ids(&mut yt, link)?;
    let id = ids.first().ok_or("YouTube Music: no album found")?;
    let album = music::album(&mut yt, id, log)?;
    let selected = select(&album, link).ok_or("YouTube Music: the song is not in its album's track list")?;
    Ok((album, selected))
}

/// Downloads a song or an album into the library layout. Falls back to a
/// plain yt-dlp download when the album lookup fails.
pub fn run(
    req: &Request,
    tools: &Tools,
    pid: &Arc<Mutex<Option<u32>>>,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(Event),
) -> Result<(), String> {
    let link = music::classify(&req.url).ok_or("not a YouTube Music link")?;
    match &link {
        Link::Artist(channel) => return queue_artist(channel, &mut on_event),
        Link::Playlist(list) => return run_playlist(req, tools, pid, cancel, list, &mut on_event),
        _ => {}
    }
    on_event(Event::Processing("Looking up album".into()));
    let lines = RefCell::new(Vec::new());
    let found = lookup(&link, &|line| lines.borrow_mut().push(line));
    for line in lines.take() {
        on_event(Event::Log(line));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    let (album, selected) = match found {
        Ok(found) => found,
        Err(e) => {
            on_event(Event::Log(format!("{e}; downloading without the library layout")));
            return job::run(req, tools, pid, on_event);
        }
    };

    if !matches!(link, Link::Song(_)) {
        on_event(Event::Collection(album_label(&album)));
    }
    let total = selected.len() as u32;
    let tracks: Vec<_> = selected.iter().enumerate().map(|(i, &t)| (t, (i as u32 + 1, total))).collect();
    match download_tracks(req, tools, pid, cancel, &album, &tracks, &mut on_event)? {
        0 => Ok(()),
        n => Err(format!("{n} of {total} tracks failed (see Log)")),
    }
}

/// Downloads tracks of an album into its library folder, skipping existing
/// files. Each entry is a track index and the `(i, n)` shown as progress.
/// Returns how many tracks failed; `Err` only when cancelled.
fn download_tracks(
    req: &Request,
    tools: &Tools,
    pid: &Arc<Mutex<Option<u32>>>,
    cancel: &AtomicBool,
    album: &Album,
    tracks: &[(usize, (u32, u32))],
    on_event: &mut impl FnMut(Event),
) -> Result<u32, String> {
    let dir = req.out_dir.join(album_dir(album));
    let ext = extension(req.audio_format);
    let mut cover = None;
    let mut failed = 0;
    for &(t, item) in tracks {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let track = &album.tracks[t];
        let path = dir.join(file_name(album, track, ext));
        let item = Some(item);
        on_event(Event::Progress { frac: None, item, title: track.title.clone() });
        if path.exists() {
            on_event(Event::Log(format!("exists, skipped: {}", path.display())));
            on_event(Event::Skipped(path));
            continue;
        }
        let Some(id) = &track.video_id else {
            on_event(Event::Log(format!("not available on YouTube Music: {}", track.title)));
            failed += 1;
            continue;
        };
        let mut forward = |event| match event {
            Event::Progress { frac, .. } => on_event(Event::Progress { frac, item, title: track.title.clone() }),
            other => on_event(other),
        };
        let track_req = Request {
            url: format!("https://music.youtube.com/watch?v={id}"),
            mode: Mode::Audio,
            audio_format: req.audio_format,
            out_dir: dir.clone(),
            browser: req.browser.clone(),
            template: Some(format!(".dropdl-{id}.%(ext)s")),
        };
        match download(&track_req, tools, pid, album, track, &path, &mut cover, &mut forward) {
            Ok(()) => on_event(Event::Done(path)),
            Err(_) if cancel.load(Ordering::Relaxed) => return Err("cancelled".into()),
            Err(e) => {
                on_event(Event::Log(format!("{}: {e}", track.title)));
                failed += 1;
            }
        }
    }
    // Folders of an album that got no file at all.
    let _ = std::fs::remove_dir(&dir);
    let _ = dir.parent().map(std::fs::remove_dir);
    Ok(failed)
}

/// Playlist items grouped by album in order of first appearance, plus the
/// items without an album (music videos, uploads), as item indexes.
fn group_by_album(items: &[ytm::PlaylistItem]) -> (Vec<(String, Vec<usize>)>, Vec<usize>) {
    let mut albums: Vec<(String, Vec<usize>)> = Vec::new();
    let mut loose = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match &item.album {
            Some(id) => match albums.iter_mut().find(|(a, _)| a == id) {
                Some((_, list)) => list.push(i),
                None => albums.push((id.clone(), vec![i])),
            },
            None => loose.push(i),
        }
    }
    (albums, loose)
}

/// Album tracks for playlist items, by video id; items not found in the
/// album are returned as unmatched.
fn match_tracks(album: &Album, items: &[ytm::PlaylistItem], indexes: &[usize]) -> (Vec<(usize, usize)>, Vec<usize>) {
    let mut found = Vec::new();
    let mut unmatched = Vec::new();
    for &i in indexes {
        let id = items[i].video_id.as_deref();
        match album.tracks.iter().position(|t| id.is_some() && t.video_id.as_deref() == id) {
            Some(t) => found.push((i, t)),
            None => unmatched.push(i),
        }
    }
    (found, unmatched)
}

/// Downloads a playlist: songs go to their album folders in the library
/// layout, everything without an album to a `<Playlist>` folder with
/// yt-dlp's metadata. Falls back to a plain download when the playlist
/// can't be read.
fn run_playlist(
    req: &Request,
    tools: &Tools,
    pid: &Arc<Mutex<Option<u32>>>,
    cancel: &AtomicBool,
    list: &str,
    on_event: &mut impl FnMut(Event),
) -> Result<(), String> {
    on_event(Event::Processing("Looking up playlist".into()));
    let mut yt = ytm::Client::new();
    let playlist = match yt.playlist(list) {
        Ok(p) => p,
        Err(e) => {
            on_event(Event::Log(format!("{e}; downloading without the library layout")));
            return job::run(req, tools, pid, on_event);
        }
    };
    let items = &playlist.items;
    let total = items.len() as u32;
    let item = |i: usize| (i as u32 + 1, total);
    on_event(Event::Collection(playlist.title.clone()));
    let (albums, mut loose) = group_by_album(items);
    on_event(Event::Log(format!(
        "{}: {total} items, {} songs from {} albums, {} without an album",
        playlist.title,
        items.len() - loose.len(),
        albums.len(),
        loose.len()
    )));

    let mut failed = 0;
    for (n, (id, indexes)) in albums.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        on_event(Event::Processing(format!("Looking up album {} of {}", n + 1, albums.len())));
        let lines = RefCell::new(Vec::new());
        let album = music::album(&mut yt, id, &|line| lines.borrow_mut().push(line));
        for line in lines.take() {
            on_event(Event::Log(line));
        }
        let album = match album {
            Ok(album) => album,
            Err(e) => {
                on_event(Event::Log(format!("{e}; saving its tracks in the playlist folder")));
                loose.extend(indexes);
                continue;
            }
        };
        let (found, unmatched) = match_tracks(&album, items, indexes);
        loose.extend(unmatched);
        let tracks: Vec<_> = found.iter().map(|&(i, t)| (t, item(i))).collect();
        failed += download_tracks(req, tools, pid, cancel, &album, &tracks, on_event)?;
    }

    loose.sort_unstable();
    let dir = req.out_dir.join(sanitize(if playlist.title.is_empty() { "Playlist" } else { &playlist.title }));
    for i in loose {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let Some(id) = &items[i].video_id else {
            on_event(Event::Log(format!("not available on YouTube Music: {}", items[i].title)));
            failed += 1;
            continue;
        };
        let title = items[i].title.clone();
        on_event(Event::Progress { frac: None, item: Some(item(i)), title: title.clone() });
        let video_req = Request {
            url: format!("https://music.youtube.com/watch?v={id}"),
            mode: Mode::Audio,
            audio_format: req.audio_format,
            out_dir: dir.clone(),
            browser: req.browser.clone(),
            template: None,
        };
        // yt-dlp skips files it already has and still reports them (quietly,
        // because of `--print`). With `--no-mtime` a file older than this
        // run already existed.
        let started = std::time::SystemTime::now();
        let existed = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|t| t < started);
        let result = job::run(&video_req, tools, pid, |event| match event {
            Event::Progress { frac, .. } => on_event(Event::Progress { frac, item: Some(item(i)), title: title.clone() }),
            Event::Done(path) if existed(&path) => on_event(Event::Skipped(path)),
            other => on_event(other),
        });
        match result {
            Ok(()) => {}
            Err(_) if cancel.load(Ordering::Relaxed) => return Err("cancelled".into()),
            Err(e) => {
                on_event(Event::Log(format!("{title}: {e}")));
                failed += 1;
            }
        }
    }
    match failed {
        0 => Ok(()),
        n => Err(format!("{n} of {total} tracks failed (see Log)")),
    }
}

/// Downloads one track to a temporary name in the album folder, tags it and
/// renames it, so a partial or untagged file never takes the final name.
/// `cover` is fetched on first use and kept for the album's other tracks.
#[allow(clippy::too_many_arguments)]
fn download(
    req: &Request,
    tools: &Tools,
    pid: &Arc<Mutex<Option<u32>>>,
    album: &Album,
    track: &Track,
    path: &Path,
    cover: &mut Option<Option<Vec<u8>>>,
    on_event: &mut impl FnMut(Event),
) -> Result<(), String> {
    let dir = &req.out_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let prefix = req.template.as_deref().and_then(|t| t.strip_suffix("%(ext)s")).unwrap_or_default();
    let mut file = None;
    let result = job::run(req, tools, pid, |event| match event {
        Event::Done(p) => file = Some(p),
        other => on_event(other),
    });
    let file = match (result, file) {
        (Ok(()), Some(file)) => file,
        (result, _) => {
            remove_temp(dir, prefix);
            return Err(result.err().unwrap_or_else(|| "yt-dlp reported no file".into()));
        }
    };

    on_event(Event::Processing("Lyrics".into()));
    let lyrics = lyrics::find(album, track, &mut |line| on_event(Event::Log(line)));
    on_event(Event::Processing("Tagging".into()));
    let cover = cover.get_or_insert_with(|| {
        let url = album.cover_url.as_deref()?;
        tags::fetch_cover(url).map_err(|e| on_event(Event::Log(e))).ok()
    });
    if let Err(e) = tags::write(&file, album, track, cover.as_deref(), lyrics.as_deref()) {
        on_event(Event::Log(format!("tags not written: {e}")));
    }
    if let Err(e) = std::fs::rename(&file, path) {
        remove_temp(dir, prefix);
        return Err(format!("{}: {e}", path.display()));
    }
    if let Some(cover) = cover
        && let Err(e) = tags::write_cover_file(dir, cover)
    {
        on_event(Event::Log(e));
    }
    Ok(())
}

/// Removes what yt-dlp left of a failed or cancelled track (`.part` files,
/// unconverted downloads).
fn remove_temp(dir: &Path, prefix: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if !prefix.is_empty() && entry.file_name().to_string_lossy().starts_with(prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::fixture;
    use crate::{musicbrainz, ytm::fixture_album};

    #[test]
    fn names() {
        assert_eq!(sanitize("CMND/CTRL"), "CMND_CTRL");
        assert_eq!(sanitize(r#"What? <A|B> "C:D" \E*"#), "What_ _A_B_ _C_D_ _E_");
        assert_eq!(sanitize("Tab\there"), "Tab_here");
        assert_eq!(sanitize("Album... "), "Album");
        assert_eq!(sanitize("   "), "_");
        assert_eq!(sanitize("CON"), "CON_");
        assert_eq!(sanitize("nul.txt"), "nul.txt_");
        assert_eq!(sanitize("Console"), "Console");
        assert_eq!(sanitize("¿Quién tú eres?"), "¿Quién tú eres_");
        let long = sanitize(&"é".repeat(100));
        assert!(long.len() <= MAX_NAME && long.chars().all(|c| c == 'é'));
    }

    #[test]
    fn layout() {
        let album = fixture_album("ram");
        assert_eq!(album_dir(&album), Path::new("Daft Punk").join("Random Access Memories (2013)"));
        let lucky = album.tracks.iter().find(|t| t.title.starts_with("Get Lucky")).unwrap();
        assert!(file_name(&album, lucky, "m4a").starts_with("08 - Get Lucky"));
        assert!(file_name(&album, lucky, "mp3").ends_with(".mp3"));

        let mut undated = album.clone();
        undated.date = None;
        assert_eq!(album_dir(&undated), Path::new("Daft Punk").join("Random Access Memories"));
        let various = fixture_album("various");
        assert!(album_dir(&various).starts_with("Various Artists"));

        let mut deftones = fixture_album("deftones");
        musicbrainz::apply(&mut deftones, &musicbrainz::parse_release(&fixture("mb_release_deftones")).unwrap());
        assert_eq!(file_name(&deftones, &deftones.tracks[11], "m4a"), "2-01 - My Own Summer (Shove It).m4a");
        assert_eq!(file_name(&deftones, &deftones.tracks[77], "m4a"), "7-11 - What Happened to You_.m4a");
    }

    #[test]
    fn selection() {
        let album = fixture_album("ram");
        assert_eq!(select(&album, &Link::Album("x".into())).unwrap().len(), 13);
        assert_eq!(select(&album, &Link::Song("zKSsP2084nU".into())), Some(vec![0]));
        assert_eq!(select(&album, &Link::Song("not-in-album".into())), None);
    }

    #[test]
    fn routing() {
        let song = "https://music.youtube.com/watch?v=zKSsP2084nU";
        assert!(handles(song, Mode::Auto) && handles(song, Mode::Audio));
        assert!(!handles(song, Mode::Video));
        assert!(handles("https://music.youtube.com/playlist?list=OLAK5uy_abc", Mode::Auto));
        assert!(handles("https://music.youtube.com/browse/MPREb_K8qWMWVqXGi", Mode::Auto));
        assert!(handles("https://music.youtube.com/playlist?list=PL123", Mode::Auto));
        assert!(handles("https://music.youtube.com/playlist?list=RDCLAK5uy_abc", Mode::Audio));
        assert!(!handles("https://music.youtube.com/playlist?list=PL123", Mode::Video));
        assert!(handles("https://music.youtube.com/channel/UCRr1xG_2WIDs18a6cIiCxeA", Mode::Auto));
        assert!(!handles("https://music.youtube.com/channel/UCRr1xG_2WIDs18a6cIiCxeA", Mode::Video));
        assert!(!handles("https://www.youtube.com/watch?v=zKSsP2084nU", Mode::Audio));
    }

    #[test]
    fn artist_links() {
        let artist = ytm::Artist { name: "A".into(), releases: vec!["MPREb_1".into(), "MPREb_2".into()] };
        assert_eq!(
            release_links(&artist).unwrap(),
            ["https://music.youtube.com/browse/MPREb_1", "https://music.youtube.com/browse/MPREb_2"]
        );
        // Every queued link goes through library mode again.
        assert!(release_links(&artist).unwrap().iter().all(|l| handles(l, Mode::Auto)));
        let empty = ytm::Artist { name: "A".into(), releases: Vec::new() };
        assert_eq!(release_links(&empty).unwrap_err(), "No albums or singles found for this artist");
    }

    fn item(video_id: Option<&str>, album: Option<&str>) -> ytm::PlaylistItem {
        ytm::PlaylistItem { video_id: video_id.map(str::to_string), title: "t".into(), album: album.map(str::to_string) }
    }

    #[test]
    fn playlist_groups() {
        let items = [
            item(Some("v1"), None),
            item(Some("a1"), Some("A")),
            item(Some("b1"), Some("B")),
            item(Some("a2"), Some("A")),
            item(None, None),
        ];
        let (albums, loose) = group_by_album(&items);
        assert_eq!(albums, [("A".to_string(), vec![1, 3]), ("B".to_string(), vec![2])]);
        assert_eq!(loose, [0, 4]);
    }

    #[test]
    fn playlist_tracks() {
        let album = fixture_album("ram");
        let lucky = album.tracks.iter().position(|t| t.title.starts_with("Get Lucky")).unwrap();
        let lucky_id = album.tracks[lucky].video_id.as_deref();
        let items = [item(Some("zKSsP2084nU"), Some("x")), item(Some("not-in-album"), Some("x")), item(lucky_id, Some("x"))];
        let (found, unmatched) = match_tracks(&album, &items, &[0, 1, 2]);
        assert_eq!(found, [(0, 0), (2, lucky)]);
        assert_eq!(unmatched, [1]);
        assert_eq!(match_tracks(&album, &[item(None, Some("x"))], &[0]), (Vec::new(), vec![0]));
    }

    #[test]
    fn temp_cleanup() {
        let dir = std::env::temp_dir().join("dropdl-library-temp");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in [".dropdl-abc.webm.part", ".dropdl-abc.m4a", ".dropdl-xyz.m4a", "01 - Song.m4a"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        remove_temp(&dir, ".dropdl-abc.");
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
        left.sort();
        assert_eq!(left, [".dropdl-xyz.m4a", "01 - Song.m4a"]);
    }
}

/// Downloads a YouTube Music song into the library layout twice: the second
/// run skips it. Needs network. Run with:
/// `cargo test library::e2e -- --ignored --nocapture` (`DROPDL_TEST_OUT`, `DROPDL_HOME` as for job::e2e).
#[cfg(test)]
mod e2e {
    use super::*;
    use lofty::prelude::*;

    #[test]
    #[ignore]
    fn song() {
        let tools = crate::job::e2e::install_tools();
        let out_dir = crate::job::e2e::out_dir().join("library");
        let req = Request {
            url: "https://music.youtube.com/watch?v=zKSsP2084nU".into(),
            mode: Mode::Auto,
            audio_format: AudioFormat::M4a,
            out_dir: out_dir.clone(),
            browser: None,
            template: None,
        };
        let path = out_dir.join("Daft Punk/Random Access Memories (2013)/01 - Give Life Back to Music.m4a");
        let _ = std::fs::remove_file(&path);
        let mut logs = Vec::new();
        // Downloaded files, then skipped ones.
        let mut run_once = || {
            let (mut files, mut skipped) = (Vec::new(), Vec::new());
            let result = run(&req, &tools, &Arc::default(), &AtomicBool::new(false), |e| match e {
                Event::Done(p) => files.push(p),
                Event::Skipped(p) => skipped.push(p),
                Event::Log(l) => {
                    println!("{l}");
                    logs.push(l);
                }
                _ => {}
            });
            assert!(result.is_ok(), "{result:?}");
            (files, skipped)
        };
        assert_eq!(run_once(), (vec![path.clone()], Vec::new()));
        let tag = lofty::read_from_path(&path).unwrap().primary_tag().unwrap().clone();
        assert_eq!(tag.get_string(ItemKey::AlbumArtist), Some("Daft Punk"));
        assert_eq!((tag.track(), tag.track_total()), (Some(1), Some(13)));
        assert_eq!(tag.pictures().len(), 1);
        assert!(tag.get_string(ItemKey::Lyrics).is_some(), "no lyrics");
        assert!(path.with_file_name("cover.jpg").exists());
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().flatten();
        assert!(leftovers.into_iter().all(|e| !e.file_name().to_string_lossy().starts_with(".dropdl-")));
        // Second run: skipped before downloading.
        assert_eq!(run_once(), (Vec::new(), vec![path.clone()]));
        assert!(logs.iter().any(|l| l.starts_with("exists, skipped")));
        println!("OK {}", path.display());
    }

    /// Release links an artist link queues.
    fn queued(channel: &str) -> Vec<String> {
        let mut links = Vec::new();
        let req = Request {
            url: format!("https://music.youtube.com/channel/{channel}"),
            mode: Mode::Auto,
            audio_format: AudioFormat::M4a,
            out_dir: "unused".into(),
            browser: None,
            template: None,
        };
        let tools = Tools { ytdlp: "unused".into(), deno: "unused".into(), ffmpeg: "unused".into() };
        let result = run(&req, &tools, &Arc::default(), &AtomicBool::new(false), |e| match e {
            Event::Enqueue(l) => links = l,
            Event::Log(l) => println!("{l}"),
            _ => {}
        });
        assert!(result.is_ok(), "{result:?}");
        links
    }

    /// Downloads a release link into the library; returns every file and how
    /// many of them already existed.
    fn download_release(tools: &Tools, url: &str, out_dir: &Path) -> (Vec<PathBuf>, usize) {
        let req = Request {
            url: url.into(),
            mode: Mode::Auto,
            audio_format: AudioFormat::M4a,
            out_dir: out_dir.into(),
            browser: None,
            template: None,
        };
        let (mut files, mut skipped) = (Vec::new(), 0);
        let result = run(&req, tools, &Arc::default(), &AtomicBool::new(false), |e| match e {
            Event::Done(p) => files.push(p),
            Event::Skipped(p) => {
                files.push(p);
                skipped += 1;
            }
            Event::Log(l) => println!("{l}"),
            _ => {}
        });
        assert!(result.is_ok(), "{url}: {result:?}");
        (files, skipped)
    }

    /// A small mixed playlist ("Downface alone", a public user playlist): 3
    /// songs from 2 albums go to the library, 3 videos to the playlist folder.
    /// A second run downloads nothing.
    #[test]
    #[ignore]
    fn playlist() {
        let tools = crate::job::e2e::install_tools();
        let out_dir = crate::job::e2e::out_dir().join("library");
        let folder = out_dir.join("Downface alone");
        let songs = [
            out_dir.join("Downface/Confidence (1998)/06 - Ocean.m4a"),
            out_dir.join("Downface/Within (2002)/02 - Cold and Overcome.m4a"),
            out_dir.join("Downface/Within (2002)/03 - Purge.m4a"),
        ];
        let _ = std::fs::remove_dir_all(&folder);
        for song in &songs {
            let _ = std::fs::remove_file(song);
        }
        let url = "https://music.youtube.com/playlist?list=PLVVKO43LKCzQ-A9Q6Wb8V23JFFRH2XJ2o";
        let (mut files, skipped) = download_release(&tools, url, &out_dir);
        assert_eq!(skipped, 0);
        files.sort();
        println!("{files:#?}");
        assert_eq!(files.len(), 6);
        assert!(songs.iter().all(|s| files.contains(s)), "songs missing");
        let videos: Vec<_> = files.iter().filter(|f| f.starts_with(&folder)).collect();
        assert_eq!(videos.len(), 3);
        assert!(videos.iter().all(|v| v.extension().is_some_and(|e| e == "m4a")));

        let started = std::time::SystemTime::now();
        let (again, skipped) = download_release(&tools, url, &out_dir);
        println!("second run: {} files in {:?}", again.len(), started.elapsed().unwrap_or_default());
        // Songs are skipped by dropdl, videos by yt-dlp: all are reported as skipped.
        assert_eq!((again.len(), skipped), (6, 6));
        let modified = |p: &PathBuf| std::fs::metadata(p).unwrap().modified().unwrap();
        assert!(files.iter().all(|f| modified(f) < started), "a file was downloaded again");
    }

    /// Two small artists: Downface (2 albums; *Within* is downloaded whole, 9
    /// tracks) and Glassjaw (3 albums, an EP and singles; one single is downloaded).
    #[test]
    #[ignore]
    fn artist() {
        let tools = crate::job::e2e::install_tools();
        let out_dir = crate::job::e2e::out_dir().join("library");

        let downface = queued("UCsbXa5iHrOr6eJN3ethzUKg");
        assert!(downface.len() >= 2, "{downface:?}");
        let mut yt = ytm::Client::new();
        let kind = |yt: &mut ytm::Client, url: &str| {
            let id = url.rsplit('/').next().unwrap();
            yt.album(id).map(|a| (a.kind, a.title)).unwrap()
        };
        let within = downface.iter().find(|u| kind(&mut yt, u).1 == "Within").expect("Within not queued");
        let dir = out_dir.join("Downface/Within (2002)");
        let _ = std::fs::remove_dir_all(&dir);
        let (files, _) = download_release(&tools, within, &out_dir);
        assert_eq!(files.len(), 9);
        assert!(files.iter().all(|f| f.starts_with(&dir) && f.exists()));
        assert!(dir.join("09 - Chavez.m4a").exists() && dir.join("cover.jpg").exists());

        let glassjaw = queued("UC7u7YhIifuby0fhcU3f4AjQ");
        let kinds: Vec<_> = glassjaw.iter().map(|u| (u.clone(), kind(&mut yt, u).0)).collect();
        assert!(kinds.iter().filter(|(_, k)| *k == music::ReleaseKind::Album).count() >= 3, "{glassjaw:?}");
        assert!(kinds.iter().any(|(_, k)| *k == music::ReleaseKind::Ep));
        let (single, _) = kinds.iter().find(|(_, k)| *k == music::ReleaseKind::Single).expect("no single");
        let (files, _) = download_release(&tools, single, &out_dir);
        assert_eq!(files.len(), 1);
        assert!(files[0].starts_with(out_dir.join("Glassjaw")), "{}", files[0].display());
        println!("OK {} and {}", dir.display(), files[0].display());
    }
}
