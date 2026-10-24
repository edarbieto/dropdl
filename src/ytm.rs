//! Minimal YouTube Music client for the web client's internal ("innertube")
//! API, plus parsers for the pages dropdl reads. The parsers are pure and
//! tested against recorded responses in tests/fixtures.

use std::time::Duration;

use serde_json::{Value, json};

use crate::music::{self, Album, ReleaseKind, Track};

const API: &str = "https://music.youtube.com/youtubei/v1/";
const CLIENT_VERSION: &str = "1.20261014.01.00";
/// Discography pages followed at most (100 releases each).
const MAX_PAGES: usize = 20;

/// An artist's name and releases (albums, singles and EPs) as browse ids.
pub struct Artist {
    pub name: String,
    pub releases: Vec<String>,
}

pub struct Client {
    agent: ureq::Agent,
    /// Anonymous session id from the first response. Continuations need it.
    /// Kept in memory only.
    visitor: Option<String>,
}

impl Client {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
        Self { agent, visitor: None }
    }

    fn call(&mut self, endpoint: &str, mut body: Value) -> Result<Value, String> {
        let err = |e: &dyn std::fmt::Display| format!("YouTube Music: {e}");
        let mut client = json!({ "clientName": "WEB_REMIX", "clientVersion": CLIENT_VERSION, "hl": "en", "gl": "US" });
        let mut req = self
            .agent
            .post(&format!("{API}{endpoint}?prettyPrint=false"))
            .header("Content-Type", "application/json")
            .header("Origin", "https://music.youtube.com");
        if let Some(v) = &self.visitor {
            client["visitorData"] = v.as_str().into();
            req = req.header("X-Goog-Visitor-Id", v.as_str());
        }
        body["context"] = json!({ "client": client });
        let mut resp = req.send(body.to_string()).map_err(|e| err(&e))?;
        let text = resp.body_mut().read_to_string().map_err(|e| err(&e))?;
        let page: Value = serde_json::from_str(&text).map_err(|e| err(&e))?;
        if self.visitor.is_none() {
            self.visitor = page["responseContext"]["visitorData"].as_str().map(str::to_string);
        }
        Ok(page)
    }

    fn browse(&mut self, browse_id: &str) -> Result<Value, String> {
        self.call("browse", json!({ "browseId": browse_id }))
    }

    /// An album with its tracks. Tracks whose song id the album page doesn't
    /// give (see `song_id`) take it from the album's playlist, which lists songs.
    pub fn album(&mut self, browse_id: &str) -> Result<Album, String> {
        let page = self.browse(browse_id)?;
        let mut album = parse_album(&page, browse_id)?;
        if album.tracks.iter().any(|t| t.video_id.is_none())
            && let Some(list) = album_playlist(&page)
        {
            fill_song_ids(&mut album, &parse_playlist(&self.browse(&format!("VL{list}"))?));
        }
        Ok(album)
    }

    /// A playlist with all its items (pages of 100 are followed).
    pub fn playlist(&mut self, list: &str) -> Result<Playlist, String> {
        let page = self.browse(&format!("VL{list}"))?;
        let title = playlist_title(&page);
        let mut items = parse_playlist(&page);
        let mut next = playlist_continuation(&page);
        for _ in 0..MAX_PAGES {
            let Some(token) = next else { break };
            let page = self.call("browse", json!({ "continuation": token }))?;
            items.extend(parse_playlist(&page));
            next = playlist_continuation(&page);
        }
        if items.is_empty() {
            return Err("YouTube Music: the playlist is empty or private".into());
        }
        Ok(Playlist { title, items })
    }

    /// The album an `OLAK5uy_…` playlist belongs to.
    pub fn album_for_playlist(&mut self, list: &str) -> Result<String, String> {
        let items = parse_playlist(&self.browse(&format!("VL{list}"))?);
        items.into_iter().find_map(|i| i.album).ok_or_else(|| "YouTube Music: playlist is not an album".into())
    }

    /// The player page of a song: its queue (with the album link) and tabs (lyrics).
    fn next(&mut self, video_id: &str) -> Result<Value, String> {
        self.call(
            "next",
            json!({
                "videoId": video_id,
                "playlistId": format!("RDAMVM{video_id}"),
                "isAudioOnly": true,
                "enablePersistentPlaylistPanel": true,
                "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
            }),
        )
    }

    /// The album of a song. None for music videos, which have no album.
    pub fn song_album(&mut self, video_id: &str) -> Result<Option<String>, String> {
        Ok(parse_song_album(&self.next(video_id)?, video_id))
    }

    /// A song's plain lyrics (YouTube Music has no timestamps for the web client).
    pub fn lyrics(&mut self, video_id: &str) -> Result<Option<String>, String> {
        let Some(id) = lyrics_id(&self.next(video_id)?) else { return Ok(None) };
        Ok(parse_lyrics(&self.browse(&id)?))
    }

    /// An artist with every album, single and EP.
    pub fn artist(&mut self, channel_id: &str) -> Result<Artist, String> {
        let page = self.browse(channel_id)?;
        let name = artist_name(&page);
        let mut ids = Vec::new();
        for shelf in parse_artist(&page) {
            let Some((browse_id, params)) = shelf.more else {
                ids.extend(shelf.ids);
                continue;
            };
            let mut page = self.call("browse", json!({ "browseId": browse_id, "params": params }))?;
            for _ in 0..MAX_PAGES {
                let (found, next) = parse_grid(&page);
                ids.extend(found);
                let Some(token) = next else { break };
                page = self.call("browse", json!({ "continuation": token }))?;
            }
        }
        let mut seen = std::collections::HashSet::new();
        ids.retain(|id| seen.insert(id.clone()));
        Ok(Artist { name, releases: ids })
    }
}

/// Joins a text object's runs.
fn text(v: &Value) -> String {
    match v["runs"].as_array() {
        Some(runs) => runs.iter().filter_map(|r| r["text"].as_str()).collect(),
        None => v["simpleText"].as_str().unwrap_or_default().to_string(),
    }
}

fn runs(v: &Value) -> &[Value] {
    v["runs"].as_array().map(Vec::as_slice).unwrap_or_default()
}

fn browse_id(v: &Value) -> Option<&str> {
    v["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()
}

fn album_id(v: &Value) -> Option<String> {
    browse_id(v).filter(|id| id.starts_with("MPREb_")).map(str::to_string)
}

/// Text of a list item's n-th column.
fn column(item: &Value, n: usize) -> &Value {
    &item["flexColumns"][n]["musicResponsiveListItemFlexColumnRenderer"]["text"]
}

fn item_duration(item: &Value) -> Option<u32> {
    parse_duration(&text(&item["fixedColumns"][0]["musicResponsiveListItemFixedColumnRenderer"]["text"]))
}

/// "4:36" or "1:02:03" → seconds.
fn parse_duration(s: &str) -> Option<u32> {
    s.split(':').try_fold(0, |acc, part| Some(acc * 60 + part.trim().parse::<u32>().ok()?))
}

/// Artist names from text runs: artists are linked to their channel, the
/// separators (", ", " & ") are not. Unlinked text counts as one artist.
fn artists(v: &Value) -> Vec<String> {
    let linked: Vec<String> = runs(v)
        .iter()
        .filter(|r| browse_id(r).is_some_and(|id| id.starts_with("UC")))
        .filter_map(|r| r["text"].as_str().map(str::to_string))
        .collect();
    match text(v) {
        t if linked.is_empty() && !t.is_empty() => vec![t],
        _ => linked,
    }
}

/// Thumbnail URLs end in a size suffix ("=w544-h544-l90-rj").
fn cover_1200(url: &str) -> String {
    match url.rfind('=') {
        Some(i) => format!("{}=w1200-h1200-l90-rj", &url[..i]),
        None => url.to_string(),
    }
}

/// Depth-first search for the first value stored under `key` that matches `pred`.
fn find<'a>(v: &'a Value, key: &str, pred: &impl Fn(&Value) -> bool) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if let Some(found) = map.get(key).filter(|f| pred(f)) {
                return Some(found);
            }
            map.values().find_map(|c| find(c, key, pred))
        }
        Value::Array(items) => items.iter().find_map(|c| find(c, key, pred)),
        _ => None,
    }
}

/// The song behind an album track. Without a login, album pages often list
/// the music video (`playlistItemData`); the "View song credits" menu entry
/// (`MPTC` + id) names the song. A few tracks lack that entry: their page id
/// is used only when it is a song (`MUSIC_VIDEO_TYPE_ATV`).
fn song_id(item: &Value) -> Option<String> {
    let credits = find(&item["menu"], "browseId", &|v| v.as_str().is_some_and(|id| id.starts_with("MPTC")));
    if let Some(id) = credits.and_then(Value::as_str) {
        return id.strip_prefix("MPTC").map(str::to_string);
    }
    let song = find(&item["overlay"], "musicVideoType", &|v| v == "MUSIC_VIDEO_TYPE_ATV").is_some();
    item["playlistItemData"]["videoId"].as_str().filter(|_| song).map(str::to_string)
}

/// The id of an album's own playlist (`OLAK5uy_…`).
fn album_playlist(page: &Value) -> Option<String> {
    let url = page["microformat"]["microformatDataRenderer"]["urlCanonical"].as_str()?;
    let list = url.split("list=").nth(1)?;
    Some(list.split('&').next().unwrap_or(list).to_string())
}

fn parse_album(page: &Value, browse_id: &str) -> Result<Album, String> {
    let root = &page["contents"]["twoColumnBrowseResultsRenderer"];
    let header = &root["tabs"][0]["tabRenderer"]["content"]["sectionListRenderer"]["contents"][0]["musicResponsiveHeaderRenderer"];
    let title = text(&header["title"]);
    if title.is_empty() {
        return Err("YouTube Music: unexpected album page".into());
    }
    let subtitle: Vec<&str> = runs(&header["subtitle"]).iter().filter_map(|r| r["text"].as_str()).collect();
    let kind = match subtitle.first() {
        Some(&"Single") => ReleaseKind::Single,
        Some(&"EP") => ReleaseKind::Ep,
        _ => ReleaseKind::Album,
    };
    let year = subtitle.iter().find(|t| t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()));
    let album_artists = artists(&header["straplineTextOne"]);
    let cover_url = header["thumbnail"]["musicThumbnailRenderer"]["thumbnail"]["thumbnails"]
        .as_array()
        .and_then(|t| t.last())
        .and_then(|t| t["url"].as_str())
        .map(cover_1200);

    let items = root["secondaryContents"]["sectionListRenderer"]["contents"][0]["musicShelfRenderer"]["contents"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let tracks = items
        .iter()
        .map(|i| &i["musicResponsiveListItemRenderer"])
        .enumerate()
        .map(|(n, item)| {
            let artists = artists(column(item, 1));
            Track {
                title: text(column(item, 0)),
                artists: if artists.is_empty() { album_artists.clone() } else { artists },
                disc: 1,
                number: text(&item["index"]).parse().unwrap_or(n as u32 + 1),
                duration: item_duration(item),
                video_id: song_id(item),
                mb_recording: None,
                mb_artists: Vec::new(),
            }
        })
        .collect();
    Ok(Album {
        title,
        compilation: album_artists == ["Various Artists"],
        artists: album_artists,
        kind,
        date: year.map(|y| y.to_string()),
        cover_url,
        browse_id: browse_id.to_string(),
        tracks,
        discs: 1,
        mb: None,
    })
}

pub struct PlaylistItem {
    /// None when the item is unavailable.
    pub video_id: Option<String>,
    pub title: String,
    /// Only songs link to their album, music videos don't.
    pub album: Option<String>,
}

pub struct Playlist {
    pub title: String,
    pub items: Vec<PlaylistItem>,
}

/// The entries of a playlist page, or of a continuation of it.
fn playlist_entries(page: &Value) -> &[Value] {
    let shelf = &page["contents"]["twoColumnBrowseResultsRenderer"]["secondaryContents"]["sectionListRenderer"]["contents"][0]["musicPlaylistShelfRenderer"];
    let more = &page["onResponseReceivedActions"][0]["appendContinuationItemsAction"]["continuationItems"];
    shelf["contents"].as_array().or(more.as_array()).map(Vec::as_slice).unwrap_or_default()
}

/// The token of the next 100 items, if there are more.
fn playlist_continuation(page: &Value) -> Option<String> {
    playlist_entries(page)
        .iter()
        .find_map(|e| e["continuationItemRenderer"]["continuationEndpoint"]["continuationCommand"]["token"].as_str())
        .map(str::to_string)
}

fn playlist_title(page: &Value) -> String {
    find(page, "musicResponsiveHeaderRenderer", &|_| true).map(|h| text(&h["title"])).unwrap_or_default()
}

fn parse_playlist(page: &Value) -> Vec<PlaylistItem> {
    playlist_entries(page)
        .iter()
        .map(|i| &i["musicResponsiveListItemRenderer"])
        .filter(|item| item.is_object())
        .map(|item| PlaylistItem {
            video_id: item["playlistItemData"]["videoId"].as_str().map(str::to_string),
            title: text(column(item, 0)),
            album: runs(column(item, 2)).iter().find_map(album_id),
        })
        .collect()
}

/// Fills missing song ids from the album playlist entry at the same position,
/// when the titles agree.
fn fill_song_ids(album: &mut Album, items: &[PlaylistItem]) {
    for (track, item) in album.tracks.iter_mut().zip(items) {
        if track.video_id.is_none() && music::normalize(&track.title) == music::normalize(&item.title) {
            track.video_id = item.video_id.clone();
        }
    }
}

fn parse_song_album(page: &Value, video_id: &str) -> Option<String> {
    let song = find(page, "playlistPanelVideoRenderer", &|r| r["videoId"].as_str() == Some(video_id))?;
    runs(&song["longBylineText"]).iter().find_map(album_id)
}

/// The browse id (`MPLY…`) of a song's lyrics tab.
fn lyrics_id(next: &Value) -> Option<String> {
    let is_lyrics = |e: &Value| e["browseId"].as_str().is_some_and(|id| id.starts_with("MPLY"));
    find(next, "browseEndpoint", &is_lyrics).and_then(|e| e["browseId"].as_str()).map(str::to_string)
}

/// The lyrics text, without the "Source: …" footer, with `\n` line breaks.
fn parse_lyrics(page: &Value) -> Option<String> {
    let shelf = find(page, "musicDescriptionShelfRenderer", &|_| true)?;
    let lyrics = text(&shelf["description"]).replace("\r\n", "\n");
    Some(lyrics.trim().to_string()).filter(|l| !l.is_empty())
}

/// An "Albums" or "Singles & EPs" shelf of an artist page.
struct Shelf {
    /// The discography page with the full list (browse id, params), if the shelf has one.
    more: Option<(String, String)>,
    ids: Vec<String>,
}

/// The name in an artist page's header (`musicImmersiveHeaderRenderer` or another kind).
fn artist_name(page: &Value) -> String {
    page["header"].as_object().and_then(|h| h.values().next()).map(|h| text(&h["title"])).unwrap_or_default()
}

fn parse_artist(page: &Value) -> Vec<Shelf> {
    let sections = page["contents"]["singleColumnBrowseResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]["sectionListRenderer"]["contents"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    sections
        .iter()
        .map(|s| &s["musicCarouselShelfRenderer"])
        .filter_map(|shelf| {
            let header = &shelf["header"]["musicCarouselShelfBasicHeaderRenderer"];
            if !matches!(text(&header["title"]).as_str(), "Albums" | "Singles & EPs") {
                return None;
            }
            let endpoint = &header["moreContentButton"]["buttonRenderer"]["navigationEndpoint"]["browseEndpoint"];
            let more = match (endpoint["browseId"].as_str(), endpoint["params"].as_str()) {
                (Some(id), Some(params)) if id.starts_with("MPAD") => Some((id.to_string(), params.to_string())),
                _ => None,
            };
            let ids = shelf["contents"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|c| album_id(&c["musicTwoRowItemRenderer"]))
                .collect();
            Some(Shelf { more, ids })
        })
        .collect()
}

/// A discography page (or its continuation): album ids and the next page's token.
fn parse_grid(page: &Value) -> (Vec<String>, Option<String>) {
    let any = |_: &Value| true;
    let Some(grid) = find(page, "gridRenderer", &any).or_else(|| find(page, "gridContinuation", &any)) else {
        return (Vec::new(), None);
    };
    let ids = grid["items"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|i| album_id(&i["musicTwoRowItemRenderer"]))
        .collect();
    let next = grid["continuations"][0]["nextContinuationData"]["continuation"].as_str().map(str::to_string);
    (ids, next)
}

/// A recorded album page: "ram", "deftones" or "various".
#[cfg(test)]
pub fn fixture_album(name: &str) -> Album {
    parse_album(&crate::music::fixture(&format!("ytm_album_{name}")), "id").unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::fixture;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("4:36"), Some(276));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn album_page() {
        let a = parse_album(&fixture("ytm_album_ram"), "MPREb_K8qWMWVqXGi").unwrap();
        assert_eq!(a.title, "Random Access Memories");
        assert_eq!(a.artists, ["Daft Punk"]);
        assert_eq!((a.kind, a.date.as_deref(), a.compilation), (ReleaseKind::Album, Some("2013"), false));
        assert!(a.cover_url.unwrap().ends_with("=w1200-h1200-l90-rj"));
        assert_eq!(a.tracks.len(), 13);
        let t = &a.tracks[0];
        assert_eq!((t.number, t.title.as_str(), t.duration), (1, "Give Life Back to Music", Some(276)));
        // Several artists come as separate runs.
        assert!(a.tracks.iter().any(|t| t.artists == ["Daft Punk", "Pharrell Williams", "Nile Rodgers"]));
        // The song, not the music video the item plays (IluRBvnYMoY).
        assert_eq!(t.video_id.as_deref(), Some("zKSsP2084nU"));
        assert!(find(&fixture("ytm_album_ram"), "videoId", &|v| v == "IluRBvnYMoY").is_some());
        assert_eq!(song_id(&json!({ "playlistItemData": { "videoId": "x" } })), None);
    }

    fn album_items(page: &mut Value) -> &mut Vec<Value> {
        let shelf = &mut page["contents"]["twoColumnBrowseResultsRenderer"]["secondaryContents"]["sectionListRenderer"]["contents"][0];
        shelf["musicShelfRenderer"]["contents"].as_array_mut().unwrap()
    }

    #[test]
    fn tracks_without_song_credits() {
        // A song without the credits entry keeps its page id.
        let mut page = fixture("ytm_album_deftones");
        let items = album_items(&mut page);
        let i = items.iter().position(|it| it.to_string().contains("MUSIC_VIDEO_TYPE_ATV")).unwrap();
        let page_id = items[i]["musicResponsiveListItemRenderer"]["playlistItemData"]["videoId"].clone();
        items[i]["musicResponsiveListItemRenderer"].as_object_mut().unwrap().remove("menu");
        let album = parse_album(&page, "id").unwrap();
        assert_eq!(album.tracks[i].video_id.as_deref(), page_id.as_str());

        // A music video without it is unknown until the album playlist fills it in.
        let mut page = fixture("ytm_album_ram");
        album_items(&mut page)[0]["musicResponsiveListItemRenderer"].as_object_mut().unwrap().remove("menu");
        let mut album = parse_album(&page, "id").unwrap();
        assert_eq!(album.tracks[0].video_id, None);
        assert_eq!(album_playlist(&page).as_deref(), Some("OLAK5uy_kNhM2yaBTOVwrcZJepB1C9P3-n5_Sfy5c"));
        fill_song_ids(&mut album, &parse_playlist(&fixture("ytm_playlist_ram")));
        assert_eq!(album.tracks[0].video_id.as_deref(), Some("zKSsP2084nU"));
    }

    #[test]
    fn empty_track_artist_is_album_artist() {
        let a = fixture_album("deftones");
        assert_eq!(a.tracks.len(), 78);
        assert!(a.tracks.iter().all(|t| t.artists == ["Deftones"]));
        assert!(a.tracks.iter().enumerate().all(|(i, t)| t.number == i as u32 + 1));
        assert_eq!(a.tracks[0].video_id.as_deref(), Some("6R0QthZqOp4"));
        assert_eq!(a.tracks[77].title, "What Happened to You?");
    }

    #[test]
    fn compilation() {
        let a = fixture_album("various");
        assert!(a.compilation);
        assert_eq!(a.artists, ["Various Artists"]);
        assert!(a.tracks.iter().all(|t| !t.artists.is_empty() && t.artists != ["Various Artists"]));
    }

    #[test]
    fn album_links() {
        let items = parse_playlist(&fixture("ytm_playlist_ram"));
        assert_eq!(items.len(), 13);
        assert_eq!(items[0].album.as_deref(), Some("MPREb_K8qWMWVqXGi"));
        let next = fixture("ytm_next_song");
        assert_eq!(parse_song_album(&next, "zKSsP2084nU").as_deref(), Some("MPREb_K8qWMWVqXGi"));
        assert_eq!(parse_song_album(&next, "other"), None);
    }

    #[test]
    fn playlists() {
        let mixed = fixture("ytm_playlist_mixed");
        assert_eq!(playlist_title(&mixed), "Downface alone");
        let items = parse_playlist(&mixed);
        assert_eq!(items.len(), 6);
        // Songs link to their album, uploaded videos don't.
        let albums: Vec<_> = items.iter().map(|i| i.album.as_deref()).collect();
        assert_eq!(albums.iter().filter(|a| a.is_some()).count(), 3);
        assert!(albums[0].is_none() && albums[2].is_some() && albums[2] == albums[3] && albums[1] != albums[2]);
        assert!(items.iter().all(|i| i.video_id.is_some()));
        assert_eq!(playlist_continuation(&mixed), None);

        let page = fixture("ytm_playlist_page");
        assert_eq!((parse_playlist(&page).len(), playlist_continuation(&page).as_deref()), (2, Some("TOKEN")));
        let more = fixture("ytm_playlist_continuation");
        assert_eq!((parse_playlist(&more).len(), playlist_continuation(&more)), (2, None));
    }

    #[test]
    fn lyrics() {
        assert_eq!(lyrics_id(&fixture("ytm_next_song")).as_deref(), Some("MPLYt_K8qWMWVqXGi-1"));
        assert_eq!(lyrics_id(&serde_json::json!({})), None);
        assert_eq!(parse_lyrics(&fixture("ytm_lyrics")).as_deref(), Some("line 1\nline 2\n\nline 4"));
        assert_eq!(parse_lyrics(&serde_json::json!({})), None);
    }

    #[test]
    fn artist_discography() {
        let page = fixture("ytm_artist");
        assert_eq!(artist_name(&page), "Daft Punk");
        assert_eq!(artist_name(&serde_json::json!({})), "");
        let shelves = parse_artist(&page);
        assert_eq!(shelves.len(), 2);
        assert!(shelves.iter().all(|s| s.more.as_ref().is_some_and(|(id, _)| id.starts_with("MPAD"))));
        let (ids, next) = parse_grid(&fixture("ytm_discography"));
        assert_eq!(ids.len(), 100);
        assert!(ids.iter().all(|id| id.starts_with("MPREb_")));
        assert!(next.is_some());
    }
}
