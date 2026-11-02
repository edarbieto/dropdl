//! Song previews. Apple's public search gives a ready 30-second clip of most
//! songs; the rest take 30 seconds of the song itself through yt-dlp, which is
//! slower. Clips stay in a temp folder, removed on exit. Rules in
//! docs/music-library.md.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::tools::command;
use crate::ytm::Hit;
use crate::{job, music};

const LOOKUP: &str = "https://itunes.apple.com/search";
const CLIP_SECONDS: u32 = 30;
/// A song and its clip may differ by this much, in seconds, and be the same song.
const LENGTH_MARGIN: i64 = 3;
const MAX_LOOKUP_BYTES: u64 = 1 << 20;
const MAX_CLIP_BYTES: u64 = 4 << 20;

pub enum Event {
    Playing,
    Ended(Result<(), String>),
}

/// The preview that is loading or playing.
pub struct Preview {
    pub id: u64,
    pub url: String,
    pub playing: bool,
    stop: Arc<AtomicBool>,
    pids: Arc<Mutex<Vec<u32>>>,
}

impl Preview {
    /// `tools` is only needed for songs without a ready clip, and on Windows.
    pub fn start(id: u64, song: Hit, tools: Option<job::Tools>, send: impl Fn(Event) + Send + 'static) -> Self {
        let (stop, pids): (Arc<AtomicBool>, Arc<Mutex<Vec<u32>>>) = Default::default();
        let (url, flag, running) = (song.url.clone(), stop.clone(), pids.clone());
        std::thread::spawn(move || {
            let run = Run { stop: flag, pids: running };
            let result = run.play(&song, tools.as_ref(), &send);
            send(Event::Ended(result));
        });
        Self { id, url, playing: false, stop, pids }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        for &pid in self.pids.lock().unwrap().iter() {
            job::kill(pid);
        }
    }
}

/// Removes the clips. A crash leaves them behind, so this also runs on start.
pub fn clear() {
    let _ = std::fs::remove_dir_all(folder());
}

fn folder() -> PathBuf {
    std::env::temp_dir().join("dropdl-previews")
}

struct Run {
    stop: Arc<AtomicBool>,
    pids: Arc<Mutex<Vec<u32>>>,
}

impl Run {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn play(&self, song: &Hit, tools: Option<&job::Tools>, send: &impl Fn(Event)) -> Result<(), String> {
        let dir = folder();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let name = clip_name(&song.url).ok_or("Preview: not a song link")?;
        let clip = dir.join(format!("{name}.m4a"));
        if !clip.exists() {
            let part = dir.join(format!("{name}.part"));
            let saved = match store_clip(song) {
                Some(bytes) => std::fs::write(&part, bytes).map_err(|e| e.to_string()),
                None => self.youtube_clip(song, tools.ok_or("Preview: tools aren't ready")?, &part),
            };
            let saved = saved.and_then(|()| std::fs::rename(&part, &clip).map_err(|e| e.to_string()));
            if saved.is_err() {
                let _ = std::fs::remove_file(&part);
            }
            if self.stopped() {
                return Ok(());
            }
            saved?;
        }
        let Some(mut player) = self.player(&clip, tools)? else { return Ok(()) };
        if self.stopped() {
            return Ok(());
        }
        let mut child = self.spawn(&mut player)?;
        send(Event::Playing);
        let status = child.wait().map_err(|e| e.to_string());
        self.forget(&child);
        if self.stopped() || status?.success() {
            return Ok(());
        }
        Err("Preview: the clip didn't play".into())
    }

    /// Starts a process that `stop` can kill.
    fn spawn(&self, cmd: &mut Command) -> Result<Child, String> {
        // `job::kill` kills a process group on Unix.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(cmd, 0);
        let child = cmd.stderr(Stdio::null()).spawn().map_err(|e| format!("Preview: {e}"))?;
        self.pids.lock().unwrap().push(child.id());
        if self.stopped() {
            job::kill(child.id());
        }
        Ok(child)
    }

    fn forget(&self, child: &Child) {
        self.pids.lock().unwrap().retain(|&pid| pid != child.id());
    }

    /// 30 seconds of the song: ffmpeg stops reading there, so the rest isn't downloaded.
    fn youtube_clip(&self, song: &Hit, tools: &job::Tools, out: &Path) -> Result<(), String> {
        let mut ytdlp = command(&tools.ytdlp);
        ytdlp
            .args(["--no-config", "--no-playlist", "--js-runtimes"])
            .arg(format!("deno:{}", tools.deno.display()))
            .arg("--ffmpeg-location")
            .arg(&tools.ffmpeg)
            .args(["-f", "bestaudio[ext=m4a]/bestaudio", "-o", "-"])
            .arg(&song.url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        let mut source = self.spawn(&mut ytdlp)?;
        let audio = source.stdout.take().ok_or("Preview: no audio")?;
        let mut ffmpeg = command(tools.ffmpeg.join(ffmpeg_name()));
        ffmpeg
            .args(["-y", "-v", "error", "-i", "pipe:0", "-ss"])
            .arg(clip_start(song.seconds).to_string())
            .args(["-t", &CLIP_SECONDS.to_string(), "-vn", "-c", "copy", "-f", "ipod"])
            .arg(out)
            .stdin(audio)
            .stdout(Stdio::null());
        let result = self.spawn(&mut ffmpeg).and_then(|mut cut| {
            let status = cut.wait().map_err(|e| e.to_string());
            self.forget(&cut);
            status
        });
        job::kill(source.id());
        let _ = source.wait();
        self.forget(&source);
        match result? {
            status if status.success() => Ok(()),
            _ => Err("Preview: couldn't get a clip of this song".into()),
        }
    }

    /// The command that plays a clip. None when it was stopped on the way.
    fn player(&self, clip: &Path, tools: Option<&job::Tools>) -> Result<Option<Command>, String> {
        if cfg!(target_os = "macos") {
            let mut afplay = command("afplay");
            afplay.arg(clip).stdin(Stdio::null()).stdout(Stdio::null());
            return Ok(Some(afplay));
        }
        if !cfg!(windows) {
            return Err("Preview: not available on this system yet".into());
        }
        // Windows plays WAV without extra libraries.
        let wav = clip.with_extension("wav");
        if !wav.exists() {
            let tools = tools.ok_or("Preview: tools aren't ready")?;
            let mut ffmpeg = command(tools.ffmpeg.join(ffmpeg_name()));
            ffmpeg.args(["-y", "-v", "error", "-i"]).arg(clip).args(["-f", "wav"]).arg(&wav).stdin(Stdio::null()).stdout(Stdio::null());
            let mut convert = self.spawn(&mut ffmpeg)?;
            let status = convert.wait().map_err(|e| e.to_string());
            self.forget(&convert);
            if self.stopped() {
                let _ = std::fs::remove_file(&wav);
                return Ok(None);
            }
            if !status?.success() {
                let _ = std::fs::remove_file(&wav);
                return Err("Preview: couldn't convert the clip".into());
            }
        }
        let mut powershell = command("powershell");
        powershell
            .args(["-NoProfile", "-NonInteractive", "-Command", "(New-Object Media.SoundPlayer $env:DROPDL_CLIP).PlaySync()"])
            .env("DROPDL_CLIP", &wav)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        Ok(Some(powershell))
    }
}

fn ffmpeg_name() -> &'static str {
    if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }
}

/// A file name from a song link's id. Ids are letters, digits, `-` and `_`.
fn clip_name(url: &str) -> Option<String> {
    let id = url.split("v=").nth(1)?.split('&').next()?;
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| id.to_string())
}

/// Starts 30 % into the song, where it has usually got going, leaving a whole clip.
fn clip_start(seconds: Option<u32>) -> u32 {
    match seconds {
        Some(s) => (s * 3 / 10).min(s.saturating_sub(CLIP_SECONDS)),
        None => CLIP_SECONDS,
    }
}

/// A title without what editions add: "(Remastered 2007)", "- Radio Edit".
fn plain_title(title: &str) -> &str {
    let end = [" (", " [", " - "].iter().filter_map(|s| title.find(s)).min();
    title[..end.unwrap_or(title.len())].trim()
}

/// The clip of `song` among Apple's results: same title, artist and length, so
/// a cover or another edit doesn't play in its place.
fn find_clip(results: &Value, song: &Hit) -> Option<String> {
    let title = music::simplify(plain_title(&song.title));
    let artist = music::simplify(&song.artist);
    let same = |r: &&Value| {
        let name = music::simplify(r["artistName"].as_str().unwrap_or_default());
        let length = r["trackTimeMillis"].as_i64().unwrap_or(0) / 1000;
        music::simplify(plain_title(r["trackName"].as_str().unwrap_or_default())) == title
            && !artist.is_empty()
            && (name.contains(&artist) || artist.contains(&name))
            && song.seconds.is_none_or(|s| (length - s as i64).abs() <= LENGTH_MARGIN)
    };
    let url = results["results"].as_array()?.iter().find(same)?["previewUrl"].as_str()?;
    clip_allowed(url).then(|| url.to_string())
}

/// Clip links come from a response, so only Apple's hosts are fetched.
fn clip_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    rest.split('/').next().unwrap_or(rest).ends_with(".apple.com")
}

/// Apple's clip of a song, if it has one. Only the artist and title are sent.
fn store_clip(song: &Hit) -> Option<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
    let term = format!("{} {}", song.artist, plain_title(&song.title));
    let mut found = agent.get(LOOKUP).query("term", &term).query("entity", "song").query("limit", "25").call().ok()?;
    let body = found.body_mut().with_config().limit(MAX_LOOKUP_BYTES).read_to_vec().ok()?;
    let url = find_clip(&serde_json::from_slice(&body).ok()?, song)?;
    let mut clip = agent.get(&url).call().ok()?;
    clip.body_mut().with_config().limit(MAX_CLIP_BYTES).read_to_vec().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::fixture;
    use crate::ytm::Kind;

    fn song(title: &str, artist: &str, seconds: Option<u32>) -> Hit {
        Hit {
            kind: Kind::Song,
            title: title.into(),
            subtitle: String::new(),
            artist: artist.into(),
            seconds,
            explicit: false,
            thumb: None,
            url: "https://music.youtube.com/watch?v=zKSsP2084nU".into(),
        }
    }

    #[test]
    fn matches_the_same_song() {
        let found = find_clip(&fixture("itunes_search"), &song("No Surprises", "Radiohead", Some(229)));
        assert!(found.is_some_and(|u| u.starts_with("https://") && u.contains(".apple.com/")));
        // Editions, accents and case.
        assert!(find_clip(&fixture("itunes_search"), &song("NO SURPRISES (Remastered 2009)", "RADIOHEAD", Some(229))).is_some());
        // The length is optional: an artist's top songs don't list it.
        assert!(find_clip(&fixture("itunes_search"), &song("No Surprises", "Radiohead", None)).is_some());
    }

    #[test]
    fn skips_covers_and_other_edits() {
        let results = fixture("itunes_search");
        assert!(find_clip(&results, &song("No Surprises", "A Tribute Band", Some(229))).is_none());
        assert!(find_clip(&results, &song("No Surprises", "Radiohead", Some(300))).is_none());
        assert!(find_clip(&results, &song("Karma Police", "Radiohead", Some(229))).is_none());
        assert!(find_clip(&results, &song("No Surprises", "", Some(229))).is_none());
    }

    #[test]
    fn clips_come_from_apple() {
        assert!(clip_allowed("https://audio-ssl.itunes.apple.com/a.m4a"));
        assert!(!clip_allowed("http://audio-ssl.itunes.apple.com/a.m4a"));
        assert!(!clip_allowed("https://evil.example/.apple.com/a.m4a"));
        assert!(!clip_allowed("https://notapple.com/a.m4a"));
    }

    #[test]
    fn titles() {
        assert_eq!(plain_title("De Música Ligera (Remasterizado 2007)"), "De Música Ligera");
        assert_eq!(plain_title("One More Time - Radio Edit"), "One More Time");
        assert_eq!(plain_title("Song [Live]"), "Song");
        assert_eq!(plain_title("(Nothing But) Flowers"), "(Nothing But) Flowers");
    }

    #[test]
    fn clip_names() {
        assert_eq!(clip_name("https://music.youtube.com/watch?v=zKS_P20-4nU").as_deref(), Some("zKS_P20-4nU"));
        assert_eq!(clip_name("https://music.youtube.com/watch?v=a1&list=b").as_deref(), Some("a1"));
        assert_eq!(clip_name("https://music.youtube.com/watch?v=../../x"), None);
        assert_eq!(clip_name("https://music.youtube.com/browse/x"), None);
    }

    #[test]
    fn clips_start_inside_the_song() {
        assert_eq!(clip_start(Some(300)), 90);
        assert_eq!(clip_start(Some(35)), 5);
        assert_eq!(clip_start(Some(20)), 0);
        assert_eq!(clip_start(None), 30);
    }

    fn run() -> Run {
        Run { stop: Arc::default(), pids: Arc::default() }
    }

    #[cfg(unix)]
    fn alive(pid: u32) -> bool {
        command("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().unwrap().success()
    }

    /// `stop` reaches a process that's already running.
    #[cfg(unix)]
    #[test]
    fn stop_kills_what_is_running() {
        let run = run();
        let mut child = run.spawn(command("sleep").arg("30")).unwrap();
        let pid = child.id();
        let preview = Preview { id: 1, url: String::new(), playing: false, stop: run.stop.clone(), pids: run.pids.clone() };
        preview.stop();
        assert!(!child.wait().unwrap().success());
        assert!(!alive(pid));
    }

    /// A process started right after `stop` doesn't get away.
    #[cfg(unix)]
    #[test]
    fn stopped_runs_kill_what_they_start() {
        let run = run();
        run.stop.store(true, Ordering::Relaxed);
        let mut child = run.spawn(command("sleep").arg("30")).unwrap();
        assert!(!child.wait().unwrap().success());
    }

    /// 30 seconds of a song through yt-dlp, without downloading the rest.
    #[test]
    #[ignore = "network"]
    fn live_youtube_clip() {
        let tools = job::e2e::install_tools();
        let out = job::e2e::out_dir().join("preview-clip.part");
        let started = std::time::Instant::now();
        run().youtube_clip(&song("Give Life Back to Music", "Daft Punk", Some(276)), &tools, &out).unwrap();
        let size = std::fs::metadata(&out).unwrap().len();
        println!("{size} bytes in {:?}", started.elapsed());
        let _ = std::fs::remove_file(&out);
        assert!((100_000..2_000_000).contains(&size), "{size} bytes");
    }

    /// Apple's clip for a real song, ready to play.
    #[test]
    #[ignore = "network"]
    fn live_store_clip() {
        let clip = store_clip(&song("No Surprises", "Radiohead", Some(229))).unwrap();
        assert!(clip.len() > 100_000, "{} bytes", clip.len());
    }
}
