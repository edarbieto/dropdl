//! Writes dropdl's own tags to downloaded M4A and MP3 files. Tag schema in
//! docs/music-library.md.

use std::io::{Cursor, Seek};
use std::path::Path;
use std::time::Duration;

use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{ItemValue, Tag, TagItem, TagType};

use crate::music::{Album, Track};

/// Larger covers are refused (YouTube Music's 1200 px JPEGs are ~200 KB).
const MAX_COVER: u64 = 10 * 1024 * 1024;

/// Downloads an album cover; it must be a JPEG.
pub fn fetch_cover(url: &str) -> Result<Vec<u8>, String> {
    let err = |e: &dyn std::fmt::Display| format!("cover: {e}");
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
    let mut resp = agent.get(url).call().map_err(|e| err(&e))?;
    let data = resp.body_mut().with_config().limit(MAX_COVER).read_to_vec().map_err(|e| err(&e))?;
    if !is_jpeg(&data) {
        return Err("cover: not a JPEG image".into());
    }
    Ok(data)
}

fn is_jpeg(data: &[u8]) -> bool {
    data.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// Writes `cover.jpg` into an album folder, keeping an existing one.
pub fn write_cover_file(dir: &Path, cover: &[u8]) -> Result<(), String> {
    let path = dir.join("cover.jpg");
    if path.exists() {
        return Ok(());
    }
    std::fs::write(&path, cover).map_err(|e| format!("{}: {e}", path.display()))
}

/// Replaces the file's tags with the album and track data, the cover and the lyrics.
pub fn write(path: &Path, album: &Album, track: &Track, cover: Option<&[u8]>, lyrics: Option<&str>) -> Result<(), String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    // Tagged in memory: lofty 0.25 panics when an MP4 write fails halfway
    // (and the release build aborts on panics), so no I/O happens inside it.
    let mut data = Cursor::new(std::fs::read(path).map_err(|e| err(&e))?);
    let file = Probe::new(&mut data).guess_file_type().map_err(|e| err(&e))?.read().map_err(|e| err(&e))?;
    data.rewind().map_err(|e| err(&e))?;
    // Saving replaces the whole tag of that type, including what yt-dlp wrote.
    let tag = build(file.primary_tag_type(), album, track, cover, lyrics);
    tag.save_to(&mut data, WriteOptions::default()).map_err(|e| err(&e))?;
    // A temporary file and a rename, so a failure never leaves a half-written file.
    let tmp = path.with_extension("tagging");
    std::fs::write(&tmp, data.into_inner()).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        err(&e)
    })
}

fn build(tag_type: TagType, album: &Album, track: &Track, cover: Option<&[u8]>, lyrics: Option<&str>) -> Tag {
    let mut tag = Tag::new(tag_type);
    let mut text = |key: ItemKey, value: &str| {
        tag.push(TagItem::new(key, ItemValue::Text(value.to_string())));
    };
    text(ItemKey::TrackTitle, &track.title);
    for artist in &track.artists {
        text(ItemKey::TrackArtist, artist);
        text(ItemKey::TrackArtists, artist);
    }
    text(ItemKey::AlbumTitle, &album.title);
    text(ItemKey::AlbumArtist, &album.artists.join(", "));
    if let Some(date) = &album.date {
        text(ItemKey::RecordingDate, date);
    }
    if let Some(lyrics) = lyrics {
        // ID3v2 drops `ItemKey::Lyrics`; MP4 maps both to ©lyr.
        text(ItemKey::UnsyncLyrics, lyrics);
    }
    if album.compilation {
        text(ItemKey::FlagCompilation, "1");
    }
    if let Some(mb) = &album.mb {
        text(ItemKey::MusicBrainzReleaseId, &mb.release);
        text(ItemKey::MusicBrainzReleaseGroupId, &mb.release_group);
        for id in &mb.artists {
            text(ItemKey::MusicBrainzReleaseArtistId, id);
        }
    }
    if let Some(id) = &track.mb_recording {
        text(ItemKey::MusicBrainzRecordingId, id);
    }
    for id in &track.mb_artists {
        text(ItemKey::MusicBrainzArtistId, id);
    }
    tag.set_track(track.number);
    tag.set_track_total(album.tracks.iter().filter(|t| t.disc == track.disc).count() as u32);
    tag.set_disk(track.disc);
    tag.set_disk_total(album.discs);
    if let Some(cover) = cover {
        let picture = Picture::unchecked(cover.to_vec()).pic_type(PictureType::CoverFront).mime_type(MimeType::Jpeg);
        tag.push_picture(picture.build());
    }
    tag
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::fixture;
    use crate::{musicbrainz, ytm};

    /// Starts like a JPEG; lofty stores cover data as given.
    const COVER: [u8; 8] = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];

    /// A copy of a silent 1 s fixture (`m4a` or `mp3`) in its own temporary folder.
    fn audio(test: &str, ext: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dropdl-tags-{test}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("track.{ext}"));
        std::fs::copy(format!("{}/tests/fixtures/audio/silence.{ext}", env!("CARGO_MANIFEST_DIR")), &path).unwrap();
        path
    }

    fn matched(name: &str) -> Album {
        let mut album = ytm::fixture_album(name);
        musicbrainz::apply(&mut album, &musicbrainz::parse_release(&fixture(&format!("mb_release_{name}"))).unwrap());
        album
    }

    fn read(path: &Path) -> Tag {
        let file = lofty::read_from_path(path).unwrap();
        assert_eq!(file.tags().len(), 1, "{}", path.display());
        assert!(file.properties().duration().as_millis() >= 900, "audio lost");
        file.primary_tag().unwrap().clone()
    }

    fn strings(tag: &Tag, key: ItemKey) -> Vec<&str> {
        tag.get_strings(key).collect()
    }

    #[test]
    fn full_tags() {
        let album = matched("ram");
        let track = album.tracks.iter().find(|t| t.title == "Get Lucky").unwrap();
        let mb = album.mb.as_ref().unwrap();
        for ext in ["m4a", "mp3"] {
            let path = audio(&format!("full-{ext}"), ext);
            // A second write replaces the first one.
            write(&path, &album, track, None, None).unwrap();
            write(&path, &album, track, Some(&COVER), Some("[00:01.00]Like the legend of the phoenix")).unwrap();
            let tag = read(&path);
            assert_eq!(strings(&tag, ItemKey::TrackTitle), ["Get Lucky"], "{ext}");
            assert_eq!(strings(&tag, ItemKey::TrackArtist), ["Daft Punk", "Pharrell Williams"], "{ext}");
            assert_eq!(strings(&tag, ItemKey::TrackArtists), ["Daft Punk", "Pharrell Williams"], "{ext}");
            assert_eq!(tag.get_string(ItemKey::AlbumTitle), Some("Random Access Memories"));
            assert_eq!(tag.get_string(ItemKey::AlbumArtist), Some("Daft Punk"));
            assert_eq!(tag.get_string(ItemKey::RecordingDate), Some("2013-05-17"), "{ext}");
            assert_eq!((tag.track(), tag.track_total(), tag.disk(), tag.disk_total()), (Some(8), Some(13), Some(1), Some(1)));
            let lyrics = tag.get_string(ItemKey::UnsyncLyrics).or(tag.get_string(ItemKey::Lyrics));
            assert_eq!(lyrics, Some("[00:01.00]Like the legend of the phoenix"), "{ext}");
            assert_eq!(tag.get_string(ItemKey::MusicBrainzReleaseId), Some(mb.release.as_str()));
            assert_eq!(tag.get_string(ItemKey::MusicBrainzReleaseGroupId), Some(mb.release_group.as_str()));
            assert_eq!(strings(&tag, ItemKey::MusicBrainzReleaseArtistId), mb.artists);
            assert_eq!(tag.get_string(ItemKey::MusicBrainzRecordingId), track.mb_recording.as_deref(), "{ext}");
            assert_eq!(strings(&tag, ItemKey::MusicBrainzArtistId), track.mb_artists);
            assert!(tag.get_string(ItemKey::FlagCompilation).is_none());
            assert_eq!(tag.pictures().len(), 1);
            if ext == "mp3" {
                // MP4 covers have no picture type.
                assert_eq!(tag.pictures()[0].pic_type(), PictureType::CoverFront);
            }
            assert_eq!(tag.pictures()[0].data(), COVER);
            assert!(!path.with_extension("tagging").exists());
        }
    }

    #[test]
    fn without_musicbrainz() {
        let album = ytm::fixture_album("ram");
        for ext in ["m4a", "mp3"] {
            let path = audio(&format!("plain-{ext}"), ext);
            write(&path, &album, &album.tracks[0], None, None).unwrap();
            let tag = read(&path);
            assert_eq!(tag.get_string(ItemKey::RecordingDate), Some("2013"), "{ext}");
            assert!(tag.get_string(ItemKey::MusicBrainzReleaseId).is_none());
            assert!(tag.get_string(ItemKey::MusicBrainzRecordingId).is_none());
            assert!(tag.get_string(ItemKey::UnsyncLyrics).or(tag.get_string(ItemKey::Lyrics)).is_none());
            assert!(tag.pictures().is_empty());
        }
    }

    #[test]
    fn multi_disc_numbers() {
        let album = matched("deftones");
        let path = audio("discs", "m4a");
        write(&path, &album, &album.tracks[11], None, None).unwrap();
        let tag = read(&path);
        // "My Own Summer (Shove It)" opens disc 2 of 7, which has 10 tracks.
        assert_eq!((tag.track(), tag.track_total(), tag.disk(), tag.disk_total()), (Some(1), Some(10), Some(2), Some(7)));
    }

    #[test]
    fn compilation() {
        let album = ytm::fixture_album("various");
        let track = &album.tracks[0];
        for ext in ["m4a", "mp3"] {
            let path = audio(&format!("various-{ext}"), ext);
            write(&path, &album, track, None, None).unwrap();
            let tag = read(&path);
            assert!(tag.get_string(ItemKey::FlagCompilation).is_some(), "{ext}");
            assert_eq!(tag.get_string(ItemKey::AlbumArtist), Some("Various Artists"));
            assert_eq!(strings(&tag, ItemKey::TrackArtist), track.artists);
        }
    }

    #[test]
    fn not_audio() {
        let path = audio("not-audio", "m4a");
        std::fs::write(&path, b"not audio").unwrap();
        let album = ytm::fixture_album("ram");
        assert!(write(&path, &album, &album.tracks[0], None, None).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not audio");
        assert!(!path.with_extension("tagging").exists());
    }

    #[test]
    fn cover_file() {
        let dir = audio("cover", "m4a").parent().unwrap().to_path_buf();
        write_cover_file(&dir, &COVER).unwrap();
        assert_eq!(std::fs::read(dir.join("cover.jpg")).unwrap(), COVER);
        // An existing cover.jpg is kept.
        write_cover_file(&dir, &[0xFF, 0xD8, 0xFF, 0]).unwrap();
        assert_eq!(std::fs::read(dir.join("cover.jpg")).unwrap(), COVER);
        assert!(is_jpeg(&COVER) && !is_jpeg(b"\x89PNG"));
    }
}
