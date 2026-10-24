//! Lyrics for a library track: LRCLIB (synced, else plain), then YouTube
//! Music's plain lyrics. Rules in docs/music-library.md.

use std::time::Duration;

use serde_json::Value;

use crate::music::{Album, Track};
use crate::ytm;

const API: &str = "https://lrclib.net/api/";
/// LRCLIB asks clients to identify themselves.
const USER_AGENT: &str = concat!("dropdl/", env!("CARGO_PKG_VERSION"), " (", env!("CARGO_PKG_REPOSITORY"), ")");
/// Allowed difference between the track and an LRCLIB search result, in seconds.
const MAX_LENGTH_DIFF: f64 = 2.0;

/// What an LRCLIB record offers.
#[derive(Debug, PartialEq)]
enum Record {
    /// LRC text with timestamps.
    Synced(String),
    Plain(String),
    Instrumental,
    /// A record without lyrics.
    Empty,
}

fn record(v: &Value) -> Record {
    let text = |key: &str| v[key].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    if v["instrumental"].as_bool() == Some(true) {
        Record::Instrumental
    } else if let Some(lrc) = text("syncedLyrics") {
        Record::Synced(lrc)
    } else if let Some(plain) = text("plainLyrics") {
        Record::Plain(plain)
    } else {
        Record::Empty
    }
}

/// The search result of about the same length, preferring synced lyrics.
fn best_match(results: &Value, duration: u32) -> Record {
    let close = |r: &&Value| r["duration"].as_f64().is_some_and(|d| (d - f64::from(duration)).abs() <= MAX_LENGTH_DIFF);
    let found: Vec<Record> = results.as_array().map(Vec::as_slice).unwrap_or_default().iter().filter(close).map(record).collect();
    let synced = found.iter().position(|r| matches!(r, Record::Synced(_)));
    let plain = found.iter().position(|r| matches!(r, Record::Plain(_)));
    match synced.or(plain) {
        Some(i) => found.into_iter().nth(i).unwrap_or(Record::Empty),
        None => Record::Empty,
    }
}

/// An LRCLIB request; `None` when LRCLIB has no such track (404).
fn get(path: &str, query: &[(&str, &str)]) -> Result<Option<Value>, String> {
    let err = |e: &dyn std::fmt::Display| format!("LRCLIB: {e}");
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
    let mut attempt = 0;
    loop {
        attempt += 1;
        let mut req = agent.get(&format!("{API}{path}")).header("User-Agent", USER_AGENT);
        for (k, v) in query {
            req = req.query(k, v);
        }
        match req.call() {
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            // 503: "The server is busy, please retry in a moment".
            Err(ureq::Error::StatusCode(503)) if attempt < 3 => std::thread::sleep(Duration::from_secs(2 * attempt)),
            Err(e) => return Err(err(&e)),
            Ok(mut resp) => {
                let text = resp.body_mut().read_to_string().map_err(|e| err(&e))?;
                return serde_json::from_str(&text).map(Some).map_err(|e| err(&e));
            }
        }
    }
}

/// LRCLIB's record for the track: an exact lookup, then a search by title
/// and artist (album names often differ, e.g. "(U.S. Version)").
fn lrclib(album: &Album, track: &Track, artist: &str) -> Result<Record, String> {
    let duration = track.duration.map(|d| d.to_string());
    let mut query = vec![("artist_name", artist), ("track_name", track.title.as_str()), ("album_name", album.title.as_str())];
    if let Some(d) = &duration {
        query.push(("duration", d));
    }
    let exact = get("get", &query)?.as_ref().map_or(Record::Empty, record);
    if exact != Record::Empty {
        return Ok(exact);
    }
    // Without a length, a search can't tell versions of a song apart.
    let Some(duration) = track.duration else { return Ok(Record::Empty) };
    let results = get("search", &[("artist_name", artist), ("track_name", track.title.as_str())])?;
    Ok(results.map_or(Record::Empty, |r| best_match(&r, duration)))
}

/// The lyrics to embed in a track: synced LRC text when LRCLIB has it, else
/// plain text from LRCLIB or YouTube Music. Problems are logged, never fatal.
pub fn find(album: &Album, track: &Track, log: &mut impl FnMut(String)) -> Option<String> {
    let artist = track.artists.first().or(album.artists.first()).map_or("", String::as_str);
    match lrclib(album, track, artist) {
        Ok(Record::Synced(lrc)) => {
            log(format!("lyrics: synced (LRCLIB): {}", track.title));
            return Some(lrc);
        }
        Ok(Record::Plain(text)) => {
            log(format!("lyrics: plain (LRCLIB): {}", track.title));
            return Some(text);
        }
        Ok(Record::Instrumental) => {
            log(format!("lyrics: instrumental (LRCLIB): {}", track.title));
            return None;
        }
        Ok(Record::Empty) => {}
        Err(e) => log(e),
    }
    let video_id = track.video_id.as_deref()?;
    match ytm::Client::new().lyrics(video_id) {
        Ok(Some(text)) => {
            log(format!("lyrics: plain (YouTube Music): {}", track.title));
            Some(text)
        }
        Ok(None) => {
            log(format!("lyrics: none found: {}", track.title));
            None
        }
        Err(e) => {
            log(e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::fixture;
    use serde_json::json;

    #[test]
    fn records() {
        let synced = record(&fixture("lrclib_get_synced"));
        assert!(matches!(&synced, Record::Synced(lrc) if lrc.starts_with("[00:01.32] line 1\n")));
        assert_eq!(record(&fixture("lrclib_get_instrumental")), Record::Instrumental);
        assert_eq!(record(&json!({"plainLyrics": "line 1", "syncedLyrics": null})), Record::Plain("line 1".into()));
        // A record that exists but has no lyrics (seen for "Downface – Lowmen", October 2026).
        assert_eq!(record(&json!({"plainLyrics": "", "syncedLyrics": null, "instrumental": false})), Record::Empty);
        assert_eq!(record(&json!({"plainLyrics": null, "syncedLyrics": " \n"})), Record::Empty);
    }

    #[test]
    fn search_by_length() {
        let shira = fixture("lrclib_search_shira");
        // 151 s (another version, synced) is too short; of the 237/238 s results the synced one wins.
        assert!(matches!(best_match(&shira, 238), Record::Synced(_)));
        assert!(matches!(best_match(&shira, 151), Record::Synced(_)));
        assert_eq!(best_match(&shira, 300), Record::Empty);
        let plain_only = json!([{"duration": 238.0, "plainLyrics": "line 1", "syncedLyrics": null}]);
        assert_eq!(best_match(&plain_only, 237), Record::Plain("line 1".into()));
        assert_eq!(best_match(&json!([]), 238), Record::Empty);
    }

    #[test]
    fn user_agent() {
        assert_eq!(USER_AGENT, format!("dropdl/{} (https://github.com/edarbieto/dropdl)", env!("CARGO_PKG_VERSION")));
    }

    /// The source order against the live services. Needs network. Run with:
    /// `cargo test lyrics::tests::live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live() {
        let album = |title: &str, artist: &str| Album {
            title: title.into(),
            artists: vec![artist.into()],
            kind: crate::music::ReleaseKind::Album,
            compilation: false,
            date: None,
            cover_url: None,
            browse_id: String::new(),
            tracks: Vec::new(),
            discs: 1,
            mb: None,
        };
        let track = |title: &str, artist: &str, duration: u32, video_id: &str| Track {
            title: title.into(),
            artists: vec![artist.into()],
            disc: 1,
            number: 1,
            duration: Some(duration),
            video_id: Some(video_id.into()),
            mb_recording: None,
            mb_artists: Vec::new(),
        };
        let mut lines = Vec::new();
        let mut log = |l: String| {
            println!("{l}");
            lines.push(l);
        };
        // Exact LRCLIB match with synced lyrics.
        let lies = find(&album("Confidence", "Downface"), &track("Lies", "Downface", 233, "0szwP1wUs28"), &mut log);
        assert!(lies.is_some_and(|l| l.starts_with('[')));
        // The album name differs on LRCLIB ("Worship and Tribute"): found by the search.
        let wt = album("Worship And Tribute (U.S. Version)", "Glassjaw");
        assert!(find(&wt, &track("Cosmopolitan Bloodloss", "Glassjaw", 190, "x"), &mut log).is_some());
        // An instrumental gets no lyrics.
        let ram = album("Random Access Memories", "Daft Punk");
        assert!(find(&ram, &track("Motherboard", "Daft Punk", 342, "x"), &mut log).is_none());
        // Not on LRCLIB at all: YouTube Music's plain lyrics.
        let none = track("Give Life Back to Music zzz", "Daft Punk", 275, "zKSsP2084nU");
        assert!(find(&ram, &none, &mut log).is_some());
        assert!(lines.last().unwrap().contains("YouTube Music"));
    }
}
