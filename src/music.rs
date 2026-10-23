//! Music library metadata: album data from YouTube Music, refined with
//! MusicBrainz when a release matches. Rules in docs/music-library.md.

use crate::{musicbrainz, ytm};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReleaseKind {
    Album,
    Ep,
    Single,
}

/// MusicBrainz IDs of a matched release.
#[derive(Clone, Debug, PartialEq)]
pub struct MbIds {
    pub release: String,
    pub release_group: String,
    pub artists: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Album {
    pub title: String,
    /// Album artist(s), identical on every track.
    pub artists: Vec<String>,
    #[allow(dead_code)] // for artist downloads (step 6)
    pub kind: ReleaseKind,
    pub compilation: bool,
    /// The year from YouTube Music, or a full date ("2016-04-07") from MusicBrainz.
    pub date: Option<String>,
    /// Square, 1200 px.
    pub cover_url: Option<String>,
    #[allow(dead_code)] // for artist downloads (step 6)
    pub browse_id: String,
    pub tracks: Vec<Track>,
    pub discs: u32,
    pub mb: Option<MbIds>,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub title: String,
    pub artists: Vec<String>,
    pub disc: u32,
    pub number: u32,
    /// Seconds.
    pub duration: Option<u32>,
    /// None when the track is unavailable on YouTube Music.
    pub video_id: Option<String>,
    pub mb_recording: Option<String>,
    /// MusicBrainz IDs of the track artists.
    pub mb_artists: Vec<String>,
}

/// What a YouTube Music link points to.
#[derive(Debug, PartialEq)]
pub enum Link {
    Album(String),
    /// An album's own playlist (`OLAK5uy_…`).
    AlbumPlaylist(String),
    Playlist(String),
    Artist(String),
    Song(String),
}

pub fn classify(url: &str) -> Option<Link> {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host.strip_prefix("www.").unwrap_or(host) != "music.youtube.com" {
        return None;
    }
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let param = |name: &str| {
        query.split('&').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('=')).map(str::to_string)
    };
    let segment = |prefix: &str| path.strip_prefix(prefix).map(|s| s.split(['/', '#']).next().unwrap_or("").to_string());
    if let Some(id) = segment("browse/").filter(|id| id.starts_with("MPREb_")) {
        Some(Link::Album(id))
    } else if let Some(id) = segment("channel/").filter(|id| id.starts_with("UC")) {
        Some(Link::Artist(id))
    } else if path == "watch" {
        param("v").map(Link::Song)
    } else if path == "playlist" {
        let list = param("list")?;
        Some(if list.starts_with("OLAK5uy_") { Link::AlbumPlaylist(list) } else { Link::Playlist(list) })
    } else {
        None
    }
}

/// Words that mark a trailing "(…)", "[…]" or " - …" as a version or guest
/// note rather than part of the title.
const VERSION_WORDS: [&str; 12] = [
    "feat", "ft.", "with ", "mono", "stereo", "remaster", "deluxe", "edition", "expanded", "anniversary", "bonus",
    "version",
];

/// A title without version and guest notes: "Respect (Mono)", "Come Together -
/// Remastered 2009" and "Get Lucky (feat. Pharrell Williams)" become "Respect",
/// "Come Together" and "Get Lucky", as MusicBrainz names them.
pub fn base_title(s: &str) -> &str {
    let mut t = s.trim();
    while let Some(i) = [" (", " [", " - "].iter().filter_map(|sep| t.rfind(sep)).max() {
        let suffix = t[i..].to_ascii_lowercase();
        if !VERSION_WORDS.iter().any(|w| suffix.contains(w)) {
            break;
        }
        t = t[..i].trim_end();
    }
    for sep in [" feat. ", " ft. "] {
        if let Some(i) = t.to_ascii_lowercase().find(sep) {
            t = &t[..i];
        }
    }
    t
}

/// The base title, simplified, for comparing titles across sources.
pub fn normalize(s: &str) -> String {
    simplify(base_title(s))
}

/// Lowercase letters and digits only, without accents, for comparing names
/// across sources ("¿Quien Tu Eres?" = "¿Quién tú eres?", "RUN DMC" = "Run‐DMC").
pub fn simplify(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric()).map(fold_accent).collect()
}

fn fold_accent(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        'ý' | 'ÿ' => 'y',
        _ => c,
    }
}

/// Fetches an album and refines it with MusicBrainz. MusicBrainz problems
/// are logged and never fail the album.
pub fn album(yt: &mut ytm::Client, browse_id: &str, log: &impl Fn(String)) -> Result<Album, String> {
    let mut album = yt.album(browse_id)?;
    match musicbrainz::find(&album) {
        Ok(Some(release)) => musicbrainz::apply(&mut album, &release),
        Ok(None) => log(format!("MusicBrainz: no match for \"{}\", using YouTube Music data", album.title)),
        Err(e) => log(e),
    }
    Ok(album)
}

/// The albums a link stands for, as browse ids: one for a song or an album,
/// every album, single and EP for an artist.
pub fn album_ids(yt: &mut ytm::Client, link: &Link) -> Result<Vec<String>, String> {
    match link {
        Link::Album(id) => Ok(vec![id.clone()]),
        Link::AlbumPlaylist(list) => yt.album_for_playlist(list).map(|id| vec![id]),
        Link::Artist(channel) => yt.artist_releases(channel),
        Link::Song(video) => match yt.song_album(video)? {
            Some(id) => Ok(vec![id]),
            None => Err("YouTube Music: this song is not part of an album".into()),
        },
        Link::Playlist(_) => Err("YouTube Music: playlists are not supported yet".into()),
    }
}

#[cfg(test)]
pub fn fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        let m = "https://music.youtube.com";
        assert_eq!(classify(&format!("{m}/browse/MPREb_K8qWMWVqXGi")), Some(Link::Album("MPREb_K8qWMWVqXGi".into())));
        assert_eq!(
            classify(&format!("{m}/playlist?list=OLAK5uy_abc&si=x")),
            Some(Link::AlbumPlaylist("OLAK5uy_abc".into()))
        );
        assert_eq!(classify(&format!("{m}/playlist?list=PL123")), Some(Link::Playlist("PL123".into())));
        assert_eq!(classify(&format!("{m}/channel/UCRr1xG_2WIDs18a6cIiCxeA")), Some(Link::Artist("UCRr1xG_2WIDs18a6cIiCxeA".into())));
        assert_eq!(classify(&format!("{m}/watch?v=zKSsP2084nU&list=RDAMVM")), Some(Link::Song("zKSsP2084nU".into())));
        assert_eq!(classify("https://www.youtube.com/watch?v=zKSsP2084nU"), None);
        assert_eq!(classify(&format!("{m}/explore")), None);
    }

    #[test]
    fn normalized_titles() {
        assert_eq!(normalize("U,U,D,D,L,R,L,R,A,B,Select,Start"), "uuddlrlrabselectstart");
        assert_eq!(normalize("What Happened to You?"), normalize("what happened to you"));
        assert_eq!(normalize("Rivière"), "riviere");
        assert_eq!(normalize("Get Lucky (feat. Pharrell Williams and Nile Rodgers)"), "getlucky");
        assert_eq!(normalize("Doin' it Right (feat. Panda Bear)"), normalize("Doin’ It Right"));
        assert_eq!(normalize("Song ft. Someone"), "song");
        assert_eq!(normalize("¿Quien Tu Eres?"), normalize("¿Quién tú eres?"));
        assert_eq!(normalize("Solo de Mi"), normalize("Solo de mí"));
        assert_eq!(normalize("RUN DMC"), normalize("Run‐DMC"));
    }

    #[test]
    fn base_titles() {
        assert_eq!(base_title("Respect (Mono)"), "Respect");
        assert_eq!(base_title("I Never Loved a Man the Way I Love You (Mono)"), "I Never Loved a Man the Way I Love You");
        assert_eq!(base_title("Come Together - Remastered 2009"), "Come Together");
        assert_eq!(base_title("Abbey Road (Super Deluxe Edition)"), "Abbey Road");
        assert_eq!(base_title("Instant Crush (feat. Julian Casablancas) [Remastered]"), "Instant Crush");
        // Only version notes are removed.
        assert_eq!(base_title("Dr. Feelgood (Love Is Serious Business) [Mono]"), "Dr. Feelgood (Love Is Serious Business)");
        assert_eq!(base_title("My Own Summer (Shove It)"), "My Own Summer (Shove It)");
        assert_eq!(base_title("MOTOMAMI +"), "MOTOMAMI +");
        assert_eq!(base_title("Awesome Mix Vol. 1 & Vol. 2"), "Awesome Mix Vol. 1 & Vol. 2");
    }

    /// Prints what dropdl finds for YouTube Music links (song, album, album
    /// playlist or artist; several separated by spaces). Needs network. Run with (PowerShell):
    /// `$env:DROPDL_TEST_MUSIC_URL = "<link>"; cargo test music::tests::show -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn show() {
        let urls = std::env::var("DROPDL_TEST_MUSIC_URL").expect("set DROPDL_TEST_MUSIC_URL");
        let mut yt = ytm::Client::new();
        let log = |line: String| println!("{line}");
        let links = urls.split_whitespace().map(|url| classify(url).expect("not a YouTube Music link"));
        let ids: Vec<String> = links.flat_map(|link| album_ids(&mut yt, &link).unwrap()).collect();
        for id in ids {
            let a = album(&mut yt, &id, &log).unwrap();
            let year = a.date.as_deref().map_or("?", |d| &d[..d.len().min(4)]);
            let mb = a.mb.as_ref().map_or("no match", |m| m.release.as_str());
            println!("\n{}/{} ({year})   [{:?}, {} disc(s), MusicBrainz: {mb}]", a.artists.join(", "), a.title, a.kind, a.discs);
            for t in &a.tracks {
                let n = if a.discs > 1 { format!("{}-{:02}", t.disc, t.number) } else { format!("{:02}", t.number) };
                let id = t.video_id.as_deref().unwrap_or("unavailable");
                println!("  {n} - {}   ({}; https://music.youtube.com/watch?v={id})", t.title, t.artists.join(", "));
            }
        }
    }

    /// The whole lookup against the live services. Needs network. Run with:
    /// `cargo test music::tests::live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live() {
        let mut yt = ytm::Client::new();
        let log = |line: String| println!("{line}");
        let song = classify("https://music.youtube.com/watch?v=zKSsP2084nU").unwrap();
        let ids = album_ids(&mut yt, &song).unwrap();
        assert_eq!(ids, ["MPREb_K8qWMWVqXGi"]);
        let ram = album(&mut yt, &ids[0], &log).unwrap();
        assert_eq!((ram.tracks.len(), ram.discs), (13, 1));
        assert!(ram.mb.is_some());
        assert_eq!(ram.tracks[0].video_id.as_deref(), Some("zKSsP2084nU"));
        let deftones = album(&mut yt, "MPREb_scSiStRYbYZ", &log).unwrap();
        assert_eq!((deftones.tracks.len(), deftones.discs), (78, 7));
        // More than one discography page (100 releases each).
        let dylan = album_ids(&mut yt, &Link::Artist("UCBqkojCXby4zGkWX86FEY7Q".into())).unwrap();
        println!("{} releases", dylan.len());
        assert!(dylan.len() > 100);
    }
}
