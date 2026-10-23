//! Music library mode: YouTube Music songs and albums become
//! `Album Artist/Album (Year)/01 - Title.ext`, tagged by dropdl. Existing
//! tracks are skipped before downloading. Rules in docs/music-library.md.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::job::{self, AudioFormat, Event, Mode, Request, Tools};
use crate::music::{self, Album, Link, Track};
use crate::{tags, ytm};

/// Longest file or folder name in bytes, to stay within Windows path limits.
const MAX_NAME: usize = 120;
/// Names Windows reserves for devices, also with an extension ("CON.txt").
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether a request goes through library mode: audio from a YouTube Music
/// song, album or album playlist. Artists and playlists come later.
pub fn handles(url: &str, mode: Mode) -> bool {
    job::wants_audio(url, mode) && matches!(music::classify(url), Some(Link::Song(_) | Link::Album(_) | Link::AlbumPlaylist(_)))
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

    let dir = req.out_dir.join(album_dir(&album));
    let ext = extension(req.audio_format);
    let total = selected.len() as u32;
    let mut cover = None;
    let mut failed = 0;
    for (i, &t) in selected.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let track = &album.tracks[t];
        let path = dir.join(file_name(&album, track, ext));
        let item = Some((i as u32 + 1, total));
        on_event(Event::Progress { frac: None, item, title: track.title.clone() });
        if path.exists() {
            on_event(Event::Log(format!("exists, skipped: {}", path.display())));
            on_event(Event::Done(path));
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
        match download(&track_req, tools, pid, &album, track, &path, &mut cover, &mut forward) {
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

    on_event(Event::Processing("Tagging".into()));
    let cover = cover.get_or_insert_with(|| {
        let url = album.cover_url.as_deref()?;
        tags::fetch_cover(url).map_err(|e| on_event(Event::Log(e))).ok()
    });
    if let Err(e) = tags::write(&file, album, track, cover.as_deref(), None) {
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
        assert!(!handles("https://music.youtube.com/playlist?list=PL123", Mode::Auto));
        assert!(!handles("https://music.youtube.com/channel/UCRr1xG_2WIDs18a6cIiCxeA", Mode::Auto));
        assert!(!handles("https://www.youtube.com/watch?v=zKSsP2084nU", Mode::Audio));
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
        let mut run_once = || {
            let mut files = Vec::new();
            let result = run(&req, &tools, &Arc::default(), &AtomicBool::new(false), |e| match e {
                Event::Done(p) => files.push(p),
                Event::Log(l) => {
                    println!("{l}");
                    logs.push(l);
                }
                _ => {}
            });
            assert!(result.is_ok(), "{result:?}");
            files
        };
        assert_eq!(run_once(), std::slice::from_ref(&path));
        let tag = lofty::read_from_path(&path).unwrap().primary_tag().unwrap().clone();
        assert_eq!(tag.get_string(ItemKey::AlbumArtist), Some("Daft Punk"));
        assert_eq!((tag.track(), tag.track_total()), (Some(1), Some(13)));
        assert_eq!(tag.pictures().len(), 1);
        assert!(path.with_file_name("cover.jpg").exists());
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().flatten();
        assert!(leftovers.into_iter().all(|e| !e.file_name().to_string_lossy().starts_with(".dropdl-")));
        // Second run: skipped before downloading.
        assert_eq!(run_once(), std::slice::from_ref(&path));
        assert!(logs.iter().any(|l| l.starts_with("exists, skipped")));
        println!("OK {}", path.display());
    }
}
