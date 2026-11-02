//! Music search: results per tab, their pages, cover thumbnails and the result
//! row. Results and covers stay in memory and go with the search. Rules in
//! docs/music-library.md.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use image::{ImageFormat, ImageReader, Limits, imageops};

use crate::ytm::{Client, Detail, Hit, Kind, SearchPage};

pub const TABS: [(Kind, &str); 4] =
    [(Kind::Song, "Songs"), (Kind::Album, "Albums"), (Kind::Artist, "Artists"), (Kind::Playlist, "Playlists")];

const COVER: f32 = 56.0;
const ROW: f32 = COVER + 6.0;
/// Centers the title and subtitle on the cover.
const TEXT_TOP: f32 = 11.0;
/// Covers keep this many pixels: 56 px rows on a 2x display.
const COVER_PIXELS: u32 = 112;
/// The zoomed cover: 360 px on a 2x display.
const ZOOM_PIXELS: u32 = 720;
const BUTTON: f32 = 26.0;
/// Album pages read at once for their song counts.
pub const MAX_DETAILS: usize = 4;
const THUMB_WORKERS: usize = 4;
const MAX_THUMB_BYTES: u64 = 1 << 20;
const MAX_THUMB_SIDE: u32 = 4096;
const THUMB_HOSTS: [&str; 3] = ["googleusercontent.com", "ggpht.com", "ytimg.com"];

#[derive(Default, PartialEq, Debug)]
pub enum State {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed(String),
}

#[derive(Default)]
pub struct Results {
    pub hits: Vec<Hit>,
    pub next: Option<String>,
    pub state: State,
}

/// An opened page. The ones read for their song count stay here too, which
/// also makes opening them instant.
pub enum Loaded {
    Loading,
    Ready(Detail),
    Failed(String),
}

pub struct Search {
    pub id: u64,
    pub query: String,
    pub tab: Kind,
    pub results: HashMap<Kind, Results>,
    pub thumbs: HashMap<String, egui::TextureHandle>,
    /// Covers of opened pages already asked for.
    pub requested: HashSet<String>,
    /// The artist whose whole discography is waiting for a yes.
    pub confirm: Option<String>,
    /// The pages opened, deepest last. Going back drops one: what's under it
    /// stays as it was, and each list keeps its scroll position.
    pub stack: Vec<Hit>,
    /// Pages by link.
    pub details: HashMap<String, Loaded>,
    /// The enlarged cover, by thumbnail link.
    pub zoom: Option<String>,
    /// Enlarged covers; None when one couldn't be loaded.
    pub zooms: HashMap<String, Option<egui::TextureHandle>>,
}

impl Search {
    pub fn new(id: u64, query: String, tab: Kind) -> Self {
        Self {
            id,
            query,
            tab,
            results: HashMap::new(),
            thumbs: HashMap::new(),
            requested: HashSet::new(),
            confirm: None,
            stack: Vec::new(),
            details: HashMap::new(),
            zoom: None,
            zooms: HashMap::new(),
        }
    }

    pub fn detail(&self, url: &str) -> Option<&Detail> {
        match self.details.get(url) {
            Some(Loaded::Ready(detail)) => Some(detail),
            _ => None,
        }
    }

    pub fn loading_details(&self) -> usize {
        self.details.values().filter(|l| matches!(l, Loaded::Loading)).count()
    }

    /// Covers of `hits` nobody has asked for yet, marking them as asked.
    pub fn missing_thumbs<'a>(&mut self, hits: impl Iterator<Item = &'a Hit>) -> Vec<String> {
        let mut urls = Vec::new();
        for url in hits.filter_map(|h| h.thumb.as_ref()) {
            if !self.thumbs.contains_key(url) && self.requested.insert(url.clone()) {
                urls.push(url.clone());
            }
        }
        urls
    }

    /// Adds a page. Pages can repeat results, and one that adds nothing ends the list.
    pub fn add_page(&mut self, kind: Kind, page: Result<SearchPage, String>) {
        let results = self.results.entry(kind).or_default();
        let page = match page {
            Ok(page) => page,
            Err(e) => {
                results.state = State::Failed(e);
                return;
            }
        };
        let before = results.hits.len();
        for hit in page.hits {
            if !results.hits.iter().any(|h| h.url == hit.url) {
                results.hits.push(hit);
            }
        }
        results.next = page.next.filter(|_| results.hits.len() > before);
        results.state = State::Ready;
    }
}

pub enum Event {
    Page(Kind, Result<SearchPage, String>),
    Thumb(String, egui::TextureHandle),
    Detail(String, Result<Detail, String>),
    Zoom(String, Option<egui::TextureHandle>),
}

pub struct Task {
    pub id: u64,
    pub query: String,
    pub kind: Kind,
    /// The next page's token; the first page without one.
    pub token: Option<String>,
}

/// Loads a page and then its covers in the background. `live` holds the id of
/// the current search: anything older stops.
pub fn spawn(
    task: Task,
    client: Arc<Mutex<Client>>,
    live: Arc<AtomicU64>,
    ctx: egui::Context,
    send: impl Fn(Event) + Clone + Send + 'static,
) {
    std::thread::spawn(move || {
        let page = {
            let mut client = client.lock().unwrap();
            if live.load(Ordering::Relaxed) != task.id {
                return;
            }
            match &task.token {
                Some(token) => client.search_more(token, task.kind),
                None => client.search(&task.query, task.kind),
            }
        };
        let thumbs = page.iter().flat_map(|p| &p.hits).filter_map(|h| h.thumb.clone()).collect();
        send(Event::Page(task.kind, page));
        fetch_thumbs(thumbs, task.id, live, ctx, send);
    });
}

/// Reads a page. It gets its own client: several run at once, and pages don't
/// need the search's session.
pub fn spawn_detail(
    hit: Hit,
    id: u64,
    live: Arc<AtomicU64>,
    send: impl Fn(Event) + Send + 'static,
) {
    std::thread::spawn(move || {
        if live.load(Ordering::Relaxed) == id {
            send(Event::Detail(hit.url.clone(), Client::new().detail(&hit)));
        }
    });
}

/// Loads a large copy of a cover.
pub fn spawn_zoom(url: String, ctx: egui::Context, send: impl Fn(Event) + Send + 'static) {
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
        let image = fetch_image(&agent, &large_cover(&url), ZOOM_PIXELS);
        let texture = image.map(|image| ctx.load_texture(format!("zoom {url}"), image, egui::TextureOptions::LINEAR));
        send(Event::Zoom(url, texture));
    });
}

/// Google's image links choose their size in a suffix ("=w120-h120-l90-rj").
fn large_cover(url: &str) -> String {
    match url.rfind('=') {
        Some(i) if !url.contains("ytimg.com") => format!("{}=w{ZOOM_PIXELS}-h{ZOOM_PIXELS}-l90-rj", &url[..i]),
        _ => url.to_string(),
    }
}

/// Downloads covers, top of the list first. A cover that fails keeps its placeholder.
pub fn fetch_thumbs(
    mut urls: Vec<String>,
    id: u64,
    live: Arc<AtomicU64>,
    ctx: egui::Context,
    send: impl Fn(Event) + Clone + Send + 'static,
) {
    urls.reverse();
    let queue = Arc::new(Mutex::new(urls));
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
    for _ in 0..THUMB_WORKERS {
        let (queue, agent, live, ctx, send) = (queue.clone(), agent.clone(), live.clone(), ctx.clone(), send.clone());
        std::thread::spawn(move || {
            while live.load(Ordering::Relaxed) == id {
                let Some(url) = queue.lock().unwrap().pop() else { return };
                if let Some(image) = fetch_image(&agent, &url, COVER_PIXELS) {
                    let texture = ctx.load_texture(url.clone(), image, egui::TextureOptions::LINEAR);
                    send(Event::Thumb(url, texture));
                }
            }
        });
    }
}

fn fetch_image(agent: &ureq::Agent, url: &str, pixels: u32) -> Option<egui::ColorImage> {
    if !thumb_allowed(url) {
        return None;
    }
    let mut resp = agent.get(url).call().ok()?;
    decode(&resp.body_mut().with_config().limit(MAX_THUMB_BYTES).read_to_vec().ok()?, pixels)
}

/// Thumbnail URLs come from a response, so only Google's image hosts are fetched.
fn thumb_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split('/').next().unwrap_or(rest);
    THUMB_HOSTS.iter().any(|h| host.ends_with(&format!(".{h}")))
}

/// A square cover from a JPEG. Playlist thumbnails aren't always square, or small.
fn decode(bytes: &[u8], pixels: u32) -> Option<egui::ColorImage> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_THUMB_SIDE);
    limits.max_image_height = Some(MAX_THUMB_SIDE);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Jpeg);
    reader.limits(limits);
    let mut image = reader.decode().ok()?.to_rgba8();
    let (w, h) = image.dimensions();
    let side = w.min(h);
    let square = imageops::crop(&mut image, (w - side) / 2, (h - side) / 2, side, side).to_image();
    let cover = if side > pixels { imageops::thumbnail(&square, pixels, pixels) } else { square };
    let size = [cover.width() as usize, cover.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, cover.as_raw()))
}

pub enum Click {
    Add,
    Ask,
    No,
    Open,
    Zoom,
    Play,
    Stop,
}

/// A row's preview button.
pub enum Play {
    /// This kind of result has no preview.
    None,
    /// Can't play: why not.
    Off(String),
    /// Can play: what.
    Ready(String),
    Loading,
    Playing,
}

/// What a result row shows besides the result itself.
pub struct Row<'a> {
    pub thumb: Option<&'a egui::TextureHandle>,
    pub added: bool,
    pub asking: bool,
    pub play: Play,
    /// Added to the subtitle: an album's song count.
    pub note: &'a str,
    /// Shows the button that opens the result's page.
    pub open: bool,
}

/// One result: cover, title, subtitle and its actions. Also says whether it
/// was on screen: covers and song counts are only worth loading then.
pub fn row(ui: &mut egui::Ui, hit: &Hit, row: Row) -> (bool, Option<Click>) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return (false, None);
    }
    let mut click = None;
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.4));
    }

    let inner = rect.shrink2(egui::vec2(4.0, 3.0));
    let cover = egui::Rect::from_min_size(inner.min, egui::Vec2::splat(COVER));
    let radius = if hit.kind == Kind::Artist { COVER / 2.0 } else { 3.0 };
    match row.thumb {
        Some(texture) => {
            egui::Image::new(texture).corner_radius(radius).paint_at(ui, cover);
        }
        None => {
            ui.painter().rect_filled(cover, radius, ui.visuals().widgets.inactive.bg_fill);
        }
    }

    let available = !hit.url.is_empty();
    let action_w = if row.asking {
        150.0
    } else {
        let download = if row.added { 60.0 } else { BUTTON };
        let others = [hit.thumb.is_some(), !matches!(row.play, Play::None), row.open];
        download + BUTTON * others.iter().filter(|&&b| b).count() as f32
    };
    let action = egui::Rect::from_min_max(egui::pos2(inner.right() - action_w, inner.top()), inner.max);
    let mut ui_action = ui.new_child(
        egui::UiBuilder::new().max_rect(action).layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if row.asking {
        if ui_action.small_button("No").clicked() {
            click = Some(Click::No);
        }
        if ui_action.small_button("Yes").clicked() {
            click = Some(Click::Add);
        }
        ui_action.label(egui::RichText::new("All releases?").small());
    } else {
        if !available {
            ui_action.label(egui::RichText::new("unavailable").small().weak());
        } else if row.added {
            ui_action.label(egui::RichText::new("✔ added").small().weak());
        } else if ui_action.small_button("⬇").on_hover_text("Download").clicked() {
            click = Some(if hit.kind == Kind::Artist { Click::Ask } else { Click::Add });
        }
        if row.open && ui_action.small_button("➡").on_hover_text("Open").clicked() {
            click = Some(Click::Open);
        }
        match &row.play {
            Play::None => {}
            Play::Off(why) => {
                ui_action.add_enabled(false, egui::Button::new("⏵").small()).on_disabled_hover_text(why);
            }
            Play::Ready(what) => {
                if ui_action.small_button("⏵").on_hover_text(what).clicked() {
                    click = Some(Click::Play);
                }
            }
            Play::Loading => {
                if ui_action.add(egui::Spinner::new().size(14.0)).on_hover_text("Stop").interact(egui::Sense::click()).clicked() {
                    click = Some(Click::Stop);
                }
            }
            Play::Playing => {
                if ui_action.small_button("⏹").on_hover_text("Stop").clicked() {
                    click = Some(Click::Stop);
                }
            }
        }
        if hit.thumb.is_some() && ui_action.small_button("👁").on_hover_text("View cover").clicked() {
            click = Some(Click::Zoom);
        }
    }

    let text = egui::Rect::from_min_max(
        egui::pos2(cover.right() + 9.0, inner.top() + TEXT_TOP),
        egui::pos2(action.left() - 6.0, inner.bottom()),
    );
    let mut ui_text = ui.new_child(egui::UiBuilder::new().max_rect(text).layout(egui::Layout::top_down(egui::Align::Min)));
    ui_text.spacing_mut().item_spacing.y = 3.0;
    ui_text.spacing_mut().interact_size.y = 0.0;
    let strong = ui_text.visuals().strong_text_color();
    ui_text.add(egui::Label::new(egui::RichText::new(&hit.title).color(strong)).truncate().selectable(false));
    ui_text.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if hit.explicit {
            let bg = ui.visuals().widgets.inactive.bg_fill;
            ui.label(egui::RichText::new(" E ").small().background_color(bg)).on_hover_text("Explicit");
        }
        let subtitle = match (hit.subtitle.is_empty(), row.note.is_empty()) {
            (_, true) => hit.subtitle.clone(),
            (true, false) => row.note.to_string(),
            (false, false) => format!("{} · {}", hit.subtitle, row.note),
        };
        ui.add(egui::Label::new(egui::RichText::new(subtitle).small().weak()).truncate().selectable(false));
    });
    (true, click)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tab against the live service: results, links, covers and a second page.
    #[test]
    #[ignore = "network"]
    fn live() {
        let mut client = Client::new();
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
        for (kind, name) in TABS {
            let first = client.search("daft punk", kind).unwrap();
            assert!(!first.hits.is_empty(), "{name}: no results");
            assert!(first.hits.iter().all(|h| crate::music::classify(&h.url).is_some()), "{name}: unknown link");
            let urls: Vec<_> = first.hits.iter().filter_map(|h| h.thumb.clone()).collect();
            let covers = urls.iter().filter(|u| fetch_image(&agent, u, COVER_PIXELS).is_some()).count();
            println!("{name}: {} results, {covers}/{} covers", first.hits.len(), urls.len());
            assert!(covers > 0, "{name}: no covers");
            let hits = first.hits.len();
            let next = first.next.clone();
            let mut search = Search::new(1, "daft punk".into(), kind);
            search.add_page(kind, Ok(first));
            if let Some(token) = next {
                search.add_page(kind, client.search_more(&token, kind));
                let total = search.results[&kind].hits.len();
                println!("{name}: {total} results after page 2");
                assert!(total > hits, "{name}: no new results");
            }
        }
    }

    /// Opens a page of each kind on the live service, down to a song.
    #[test]
    #[ignore = "network"]
    fn live_pages() {
        let mut client = Client::new();
        let mut open_first = |kind| {
            let hit = client.search("daft punk", kind).unwrap().hits.remove(0);
            client.detail(&hit).unwrap()
        };
        let album = open_first(Kind::Album);
        println!("album: {}, top song: {:?}", album.summary, album.top.as_ref().map(|t| &t.title));
        assert!(album.summary.contains("song") && album.top.is_some());
        assert!(album.sections[0].hits.iter().any(|h| crate::music::classify(&h.url).is_some()));

        let artist = open_first(Kind::Artist);
        for section in &artist.sections {
            println!("artist: {} ({}, more: {})", section.title, section.hits.len(), section.more.is_some());
        }
        let playlist = open_first(Kind::Playlist);
        println!("playlist: {} ({})", playlist.summary, playlist.sections[0].hits.len());
        assert!(!playlist.sections[0].hits.is_empty());

        let mut client = Client::new();
        let albums = artist.sections.iter().find(|s| s.title == "Albums").expect("no albums");
        let all = client.detail(albums.more.as_ref().expect("no full list")).unwrap();
        println!("all albums: {}", all.sections[0].hits.len());
        assert!(all.sections[0].hits.len() >= albums.hits.len());
        let release = client.detail(&all.sections[0].hits[0]).unwrap();
        assert!(!release.sections[0].hits.is_empty());
        let songs = artist.sections.iter().find(|s| s.title == "Top songs").expect("no top songs");
        let top = client.detail(songs.more.as_ref().expect("no full list")).unwrap();
        println!("top songs: {} {}", top.summary, top.sections[0].hits.len());
        assert!(!top.sections[0].hits.is_empty());
    }

    fn hit(url: &str) -> Hit {
        Hit {
            kind: Kind::Song,
            title: url.into(),
            subtitle: String::new(),
            artist: String::new(),
            seconds: None,
            explicit: false,
            thumb: None,
            url: url.into(),
        }
    }

    fn page(urls: &[&str], next: Option<&str>) -> Result<SearchPage, String> {
        Ok(SearchPage { hits: urls.iter().map(|u| hit(u)).collect(), next: next.map(str::to_string) })
    }

    fn urls(search: &Search) -> Vec<&str> {
        search.results[&Kind::Song].hits.iter().map(|h| h.url.as_str()).collect()
    }

    #[test]
    fn pages_drop_repeats() {
        let mut search = Search::new(1, "q".into(), Kind::Song);
        search.add_page(Kind::Song, page(&["a", "b"], Some("t1")));
        search.add_page(Kind::Song, page(&["b", "c"], Some("t2")));
        assert_eq!(urls(&search), ["a", "b", "c"]);
        assert_eq!(search.results[&Kind::Song].next.as_deref(), Some("t2"));
    }

    #[test]
    fn page_without_new_results_ends_the_list() {
        let mut search = Search::new(1, "q".into(), Kind::Song);
        search.add_page(Kind::Song, page(&["a"], Some("t1")));
        search.add_page(Kind::Song, page(&["a"], Some("t2")));
        assert_eq!(search.results[&Kind::Song].next, None);
    }

    #[test]
    fn failed_page_keeps_results_and_token() {
        let mut search = Search::new(1, "q".into(), Kind::Song);
        search.add_page(Kind::Song, page(&["a"], Some("t1")));
        search.add_page(Kind::Song, Err("offline".into()));
        let results = &search.results[&Kind::Song];
        assert_eq!(results.hits.len(), 1);
        assert_eq!(results.next.as_deref(), Some("t1"));
        assert_eq!(results.state, State::Failed("offline".into()));
    }

    #[test]
    fn thumb_hosts() {
        assert!(thumb_allowed("https://yt3.googleusercontent.com/abc=w120-h120-l90-rj"));
        assert!(thumb_allowed("https://i.ytimg.com/vi/abc/sddefault.jpg"));
        assert!(!thumb_allowed("http://yt3.googleusercontent.com/abc"));
        assert!(!thumb_allowed("https://evil.example/yt3.googleusercontent.com/abc"));
        assert!(!thumb_allowed("https://notgoogleusercontent.com/abc"));
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let rgb = vec![128; (width * height * 3) as usize];
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut out)
            .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    #[test]
    fn covers_are_square_and_small() {
        assert_eq!(decode(&jpeg(40, 20), COVER_PIXELS).unwrap().size, [20, 20]);
        assert_eq!(decode(&jpeg(853, 480), COVER_PIXELS).unwrap().size, [112, 112]);
        assert_eq!(decode(&jpeg(1200, 1200), ZOOM_PIXELS).unwrap().size, [720, 720]);
    }

    #[test]
    fn bad_covers_are_skipped() {
        assert!(decode(b"not a jpeg", COVER_PIXELS).is_none());
        assert!(decode(&jpeg(40, 20)[..64], COVER_PIXELS).is_none());
        assert!(decode(&jpeg(4097, 8), COVER_PIXELS).is_none());
    }

    #[test]
    fn large_covers() {
        assert_eq!(large_cover("https://yt3.googleusercontent.com/abc=w120-h120-l90-rj"), "https://yt3.googleusercontent.com/abc=w720-h720-l90-rj");
        assert_eq!(large_cover("https://i.ytimg.com/vi/abc/sddefault.jpg?sqp=x&rs=y"), "https://i.ytimg.com/vi/abc/sddefault.jpg?sqp=x&rs=y");
    }

    #[test]
    fn covers_are_asked_for_once() {
        let mut search = Search::new(1, "q".into(), Kind::Song);
        let hits = [Hit { thumb: Some("a".into()), ..hit("x") }, Hit { thumb: Some("a".into()), ..hit("y") }, hit("z")];
        assert_eq!(search.missing_thumbs(hits.iter()), ["a"]);
        assert!(search.missing_thumbs(hits.iter()).is_empty());
    }
}
