//! MusicBrainz lookup: finds the release that matches a YouTube Music album
//! track by track. Matching rules in docs/music-library.md.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::music::{self, Album, MbIds};

const API: &str = "https://musicbrainz.org/ws/2/";
/// MusicBrainz asks for an app name, version and contact.
const USER_AGENT: &str = concat!("dropdl/", env!("CARGO_PKG_VERSION"), " ( ", env!("CARGO_PKG_REPOSITORY"), " )");
/// Release details fetched per album at most (one request each).
const MAX_CANDIDATES: usize = 3;
/// Allowed difference between YouTube Music and MusicBrainz track lengths.
const MAX_LENGTH_DIFF: u32 = 3;

pub struct Release {
    pub id: String,
    pub release_group: String,
    pub title: String,
    /// The original release date of the album (release group), not of this edition.
    pub date: Option<String>,
    pub artists: Vec<Credit>,
    pub discs: Vec<Vec<MbTrack>>,
}

pub struct Credit {
    pub name: String,
    pub id: String,
}

pub struct MbTrack {
    pub title: String,
    pub artists: Vec<Credit>,
    /// Seconds.
    pub length: Option<u32>,
    pub recording: String,
}

/// Waits until a second has passed since the previous request (MusicBrainz's rate limit).
fn wait_turn() {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = *last {
        std::thread::sleep(Duration::from_millis(1100).saturating_sub(t.elapsed()));
    }
    *last = Some(Instant::now());
}

fn get(path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let err = |e: &dyn std::fmt::Display| format!("MusicBrainz: {e}");
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
    let mut attempt = 0;
    loop {
        attempt += 1;
        wait_turn();
        let mut req = agent.get(&format!("{API}{path}")).header("User-Agent", USER_AGENT).query("fmt", "json");
        for (k, v) in query {
            req = req.query(k, v);
        }
        match req.call() {
            // 503 means "slow down": the service is busy or requests came too fast.
            Err(ureq::Error::StatusCode(503)) if attempt < 3 => std::thread::sleep(Duration::from_secs(2 * attempt)),
            Err(e) => return Err(err(&e)),
            Ok(mut resp) => {
                let text = resp.body_mut().read_to_string().map_err(|e| err(&e))?;
                return serde_json::from_str(&text).map_err(|e| err(&e));
            }
        }
    }
}

/// The release that matches `album`, if any. Searches from strict to loose,
/// and stops after `MAX_CANDIDATES` release lookups in total.
pub fn find(album: &Album) -> Result<Option<Release>, String> {
    let Some(artist) = album.artists.first() else { return Ok(None) };
    let base = music::base_title(&album.title);
    let mut queries = vec![format!("artist:\"{}\" AND release:\"{}\"", quote(artist), quote(&album.title))];
    if base != album.title {
        // "I Never Loved a Man the Way I Love You (Mono)" is not a title on MusicBrainz.
        queries.push(format!("artist:\"{}\" AND release:\"{}\"", quote(artist), quote(base)));
    }
    // Artist names can differ in punctuation ("RUN DMC", "Run‐DMC"): candidates() checks the artist.
    queries.push(format!("release:\"{}\"", quote(base)));

    let mut tried = Vec::new();
    for query in queries {
        let search = get("release", &[("query", &query), ("limit", "100")])?;
        for id in candidates(&search, album.tracks.len(), artist) {
            if tried.len() == MAX_CANDIDATES {
                return Ok(None);
            }
            if tried.contains(&id) {
                continue;
            }
            let release = parse_release(&get(&format!("release/{id}"), &[("inc", "recordings+artist-credits+release-groups")])?)?;
            if matches(album, &release) {
                return Ok(Some(release));
            }
            tried.push(id);
        }
    }
    Ok(None)
}

/// Escapes text for a quoted search term.
fn quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Releases by `artist` with `tracks` tracks in total, best first: digital, then
/// CD, then anything else (vinyl splits albums into sides), then the earliest date.
fn candidates(search: &Value, tracks: usize, artist: &str) -> Vec<String> {
    let rank = |format: Option<&str>| match format {
        Some("Digital Media") => 0,
        Some("CD") => 1,
        _ => 2,
    };
    let mut found: Vec<(u8, &str, &str)> = search["releases"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|r| r["track-count"].as_u64() == Some(tracks as u64))
        .filter(|r| by_artist(r, artist))
        .filter_map(|r| {
            let media = r["media"].as_array()?;
            let worst = media.iter().map(|m| rank(m["format"].as_str())).max()?;
            Some((worst, r["date"].as_str().filter(|d| !d.is_empty()).unwrap_or("9999"), r["id"].as_str()?))
        })
        .collect();
    found.sort();
    found.into_iter().take(MAX_CANDIDATES).map(|(_, _, id)| id.to_string()).collect()
}

/// Whether a release's artist credit ("Daft Punk", "Run‐DMC", "A & B") names `artist`.
fn by_artist(release: &Value, artist: &str) -> bool {
    let credits = release["artist-credit"].as_array().map(Vec::as_slice).unwrap_or_default();
    let credited: String =
        credits.iter().map(|c| format!("{}{}", c["name"].as_str().unwrap_or_default(), c["joinphrase"].as_str().unwrap_or_default())).collect();
    music::simplify(&credited).contains(&music::simplify(artist))
}

fn credits(v: &Value) -> Vec<Credit> {
    let credits = v.as_array().map(Vec::as_slice).unwrap_or_default();
    credits
        .iter()
        .filter_map(|c| Some(Credit { name: c["name"].as_str()?.to_string(), id: c["artist"]["id"].as_str()?.to_string() }))
        .collect()
}

pub fn parse_release(v: &Value) -> Result<Release, String> {
    let id = v["id"].as_str().ok_or("MusicBrainz: unexpected release")?;
    let discs = v["media"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m["tracks"].as_array().filter(|t| !t.is_empty()))
        .map(|tracks| {
            tracks
                .iter()
                .map(|t| MbTrack {
                    title: t["title"].as_str().unwrap_or_default().to_string(),
                    artists: credits(&t["artist-credit"]),
                    length: t["length"].as_u64().map(|ms| ((ms + 500) / 1000) as u32),
                    recording: t["recording"]["id"].as_str().unwrap_or_default().to_string(),
                })
                .collect()
        })
        .collect();
    Ok(Release {
        id: id.to_string(),
        release_group: v["release-group"]["id"].as_str().unwrap_or_default().to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        date: [&v["release-group"]["first-release-date"], &v["date"]]
            .into_iter()
            .find_map(|d| d.as_str().filter(|d| !d.is_empty()))
            .map(str::to_string),
        artists: credits(&v["artist-credit"]),
        discs,
    })
}

/// Same track count, every length within a few seconds, and at least 90% of
/// the titles equal after normalizing.
fn matches(album: &Album, release: &Release) -> bool {
    let mb: Vec<&MbTrack> = release.discs.iter().flatten().collect();
    if mb.is_empty() || mb.len() != album.tracks.len() {
        return false;
    }
    let pairs = || album.tracks.iter().zip(&mb);
    let lengths = pairs().all(|(t, m)| match (t.duration, m.length) {
        (Some(a), Some(b)) => a.abs_diff(b) <= MAX_LENGTH_DIFF,
        _ => true,
    });
    let titles = pairs().filter(|(t, m)| music::normalize(&t.title) == music::normalize(&m.title)).count();
    lengths && titles * 10 >= mb.len() * 9
}

/// Takes disc and track numbers, titles, track artists, the date and the IDs
/// from a matched release. The album artist keeps YouTube Music's spelling
/// ("RUN DMC", not "Run‐D.M.C."), also in track credits, so an artist's
/// albums stay in one folder whether they matched or not.
pub fn apply(album: &mut Album, release: &Release) {
    let own: Vec<(String, String)> = album.artists.iter().map(|a| (music::simplify(a), a.clone())).collect();
    let names = |credits: &[Credit]| {
        let spelled = |c: &Credit| own.iter().find(|(s, _)| *s == music::simplify(&c.name)).map(|(_, a)| a.clone());
        credits.iter().map(|c| spelled(c).unwrap_or_else(|| c.name.clone())).collect::<Vec<_>>()
    };
    let numbered = release
        .discs
        .iter()
        .enumerate()
        .flat_map(|(d, disc)| disc.iter().enumerate().map(move |(n, t)| (d as u32 + 1, n as u32 + 1, t)));
    for (track, (disc, number, mb)) in album.tracks.iter_mut().zip(numbered) {
        track.disc = disc;
        track.number = number;
        track.title = mb.title.clone();
        if !mb.artists.is_empty() {
            track.artists = names(&mb.artists);
            track.mb_artists = mb.artists.iter().map(|c| c.id.clone()).collect();
        }
        track.mb_recording = Some(mb.recording.clone());
    }
    album.discs = release.discs.len() as u32;
    album.title = release.title.clone();
    if release.date.is_some() {
        album.date = release.date.clone();
    }
    album.mb = Some(MbIds {
        release: release.id.clone(),
        release_group: release.release_group.clone(),
        artists: release.artists.iter().map(|c| c.id.clone()).collect(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::fixture;

    #[test]
    fn user_agent() {
        assert_eq!(USER_AGENT, format!("dropdl/{} ( https://github.com/edarbieto/dropdl )", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn quoting() {
        assert_eq!(quote(r#"A "B" \ C"#), r#"A \"B\" \\ C"#);
    }

    #[test]
    fn candidate_order() {
        let ram = candidates(&fixture("mb_search_ram"), 13, "Daft Punk");
        assert_eq!(ram.len(), MAX_CANDIDATES);
        assert_eq!(ram[0], "5000a285-b67e-4cfc-b54b-2b98f1810d2e");
        assert!(candidates(&fixture("mb_search_ram"), 13, "Someone Else").is_empty());
        let deftones = fixture("mb_search_deftones");
        assert_eq!(candidates(&deftones, 78, "DEFTONES"), ["9d69611c-2d01-411f-9e09-225698d8ef3c"]);
        assert!(candidates(&deftones, 77, "Deftones").is_empty());
    }

    #[test]
    fn single_disc_match() {
        let mut album = crate::ytm::fixture_album("ram");
        let release = parse_release(&fixture("mb_release_ram")).unwrap();
        assert!(matches(&album, &release));
        apply(&mut album, &release);
        assert_eq!((album.discs, album.date.as_deref()), (1, Some("2013-05-17")));
        assert_eq!(album.artists, ["Daft Punk"]);
        let mb = album.mb.as_ref().unwrap();
        assert_eq!(mb.release, "5000a285-b67e-4cfc-b54b-2b98f1810d2e");
        assert_eq!(mb.release_group, "aa997ea0-2936-40bd-884d-3af8a0e064dc");
        let lucky = album.tracks.iter().find(|t| t.title == "Get Lucky").unwrap();
        assert_eq!(lucky.artists, ["Daft Punk", "Pharrell Williams"]);
        assert_eq!(lucky.mb_artists.len(), 2);
        assert_eq!(lucky.mb_artists[0], mb.artists[0]);
        assert!(album.tracks.iter().all(|t| t.disc == 1 && t.mb_recording.is_some()));
    }

    #[test]
    fn keeps_album_artist_spelling() {
        let mut album = crate::ytm::fixture_album("ram");
        album.artists = vec!["DAFT PUNK".into()];
        apply(&mut album, &parse_release(&fixture("mb_release_ram")).unwrap());
        assert_eq!(album.artists, ["DAFT PUNK"]);
        let lucky = album.tracks.iter().find(|t| t.title == "Get Lucky").unwrap();
        assert_eq!(lucky.artists, ["DAFT PUNK", "Pharrell Williams"]);
    }

    #[test]
    fn multi_disc_match() {
        let mut album = crate::ytm::fixture_album("deftones");
        let release = parse_release(&fixture("mb_release_deftones")).unwrap();
        assert!(matches(&album, &release));
        apply(&mut album, &release);
        assert_eq!(album.discs, 7);
        let at = |i: usize| (album.tracks[i].disc, album.tracks[i].number, album.tracks[i].title.as_str());
        assert_eq!(at(0), (1, 1, "Bored"));
        assert_eq!(at(11), (2, 1, "My Own Summer (Shove It)"));
        assert_eq!(at(77), (7, 11, "What Happened to You?"));
        // Video ids are kept.
        assert_eq!(album.tracks[0].video_id.as_deref(), Some("6R0QthZqOp4"));
    }

    #[test]
    fn no_match() {
        let ram = parse_release(&fixture("mb_release_ram")).unwrap();
        assert!(!matches(&crate::ytm::fixture_album("deftones"), &ram));
        let mut album = crate::ytm::fixture_album("ram");
        album.tracks[4].duration = album.tracks[4].duration.map(|d| d + MAX_LENGTH_DIFF + 1);
        assert!(!matches(&album, &ram));
        let mut album = crate::ytm::fixture_album("ram");
        for t in &mut album.tracks[..2] {
            t.title = "Something else".into();
        }
        assert!(!matches(&album, &ram));
    }
}
