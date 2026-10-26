//! Synced lyrics from LRCLIB (https://lrclib.net), a free, open lyrics database.
//!
//! Off by default. When on, the title, artist, album and duration of each new
//! track are sent to lrclib.net, and nothing else. Requests run on their own
//! thread, which sleeps on a channel between tracks.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::json::Json;
use crate::net::{self, Client};

const HOST: &str = "lrclib.net";
/// LRCLIB returns versions of a song whose length differs by more than this
/// (radio edit, live...). Their timings would not match what is playing.
const DURATION_TOLERANCE: f64 = 3.0;
const CACHE_SIZE: usize = 24;

pub struct Line {
    /// Seconds from the start of the track.
    pub time: f64,
    /// Empty for instrumental breaks.
    pub text: String,
}

pub struct Lyrics {
    pub lines: Vec<Line>,
}

impl Lyrics {
    /// Parses LRC text: `[mm:ss.xx]` tags (several per line allowed), an
    /// optional `[offset:±ms]` tag, and word-level `<mm:ss.xx>` tags, which are dropped.
    pub fn parse(lrc: &str) -> Option<Self> {
        let mut offset = 0.0;
        let mut lines = Vec::new();
        for raw in lrc.lines() {
            let mut rest = raw.trim();
            let mut times = Vec::new();
            while rest.starts_with('[') {
                let Some(end) = rest.find(']') else { break };
                let tag = &rest[1..end];
                rest = &rest[end + 1..];
                if let Some(t) = timestamp(tag) {
                    times.push(t);
                } else if let Some(ms) = tag.strip_prefix("offset:") {
                    // A positive offset makes the lyrics come sooner.
                    offset = ms.trim().parse::<f64>().unwrap_or(0.0) / 1000.0;
                }
            }
            if times.is_empty() {
                continue;
            }
            let text = strip_word_tags(rest);
            for time in times {
                lines.push(Line {
                    time,
                    text: text.clone(),
                });
            }
        }
        for line in &mut lines {
            line.time = (line.time - offset).max(0.0);
        }
        lines.sort_by(|a, b| a.time.total_cmp(&b.time));
        (!lines.is_empty()).then_some(Self { lines })
    }

    /// The line being sung at `t` seconds, or `None` before the first one.
    pub fn index_at(&self, t: f64) -> Option<usize> {
        self.lines.partition_point(|l| l.time <= t).checked_sub(1)
    }

    pub fn text(&self, index: Option<usize>) -> Option<&str> {
        self.lines.get(index?).map(|l| l.text.as_str())
    }
}

fn timestamp(tag: &str) -> Option<f64> {
    let (m, s) = tag.split_once(':')?;
    let m: u32 = m.trim().parse().ok()?;
    // Some files write `mm:ss:xx` instead of `mm:ss.xx`.
    let s: f64 = s.trim().replacen(':', ".", 1).parse().ok()?;
    (0.0..60.0).contains(&s).then_some(m as f64 * 60.0 + s)
}

fn strip_word_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        match rest[start..].find('>') {
            Some(len) if timestamp(&rest[start + 1..start + len]).is_some() => {
                out.push_str(&rest[..start]);
                rest = &rest[start + len + 1..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = &rest[start + 1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Clone)]
pub struct Query {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Seconds; 0 when the player has not reported it.
    pub duration: f64,
}

impl Query {
    fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.title.to_lowercase(),
            self.artist.to_lowercase(),
            self.album.to_lowercase(),
            self.duration.round()
        )
    }
}

/// The answer for one request: its generation and the lyrics, if any were found.
pub type Answer = (u64, Option<Arc<Lyrics>>);

pub struct Fetcher {
    tx: Sender<(u64, Query)>,
    pub answer: Arc<Mutex<Option<Answer>>>,
}

impl Fetcher {
    /// Starts the worker; it posts `message` (WPARAM = generation) after each request.
    /// Dropping the fetcher ends the thread.
    pub fn start(hwnd: HWND, message: u32) -> Self {
        let (tx, rx) = mpsc::channel::<(u64, Query)>();
        let answer = Arc::new(Mutex::new(None));
        let slot = answer.clone();
        let hwnd = hwnd.0 as isize;
        let _ = std::thread::Builder::new()
            .name("lyrics".into())
            .spawn(move || {
                let agent = format!(
                    "Isla/{} (https://github.com/sirtony27/isla)",
                    env!("CARGO_PKG_VERSION")
                );
                let mut cache: Vec<(String, Option<Arc<Lyrics>>)> = Vec::new();
                while let Ok(first) = rx.recv() {
                    // Only the newest track matters when the user skips quickly.
                    let (generation, query) = rx.try_iter().last().unwrap_or(first);
                    let key = query.key();
                    let found = if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
                        let entry = cache.remove(i);
                        let found = entry.1.clone();
                        cache.push(entry);
                        found
                    } else {
                        // A connection per track, closed right after: between songs
                        // nothing stays open.
                        let client = Client::new(&agent, HOST);
                        match client.as_ref().map(|c| lookup(c, &query)) {
                            Some(Ok(found)) => {
                                cache.push((key, found.clone()));
                                if cache.len() > CACHE_SIZE {
                                    cache.remove(0);
                                }
                                found
                            }
                            // Offline or server busy: not cached, so the track is retried next time.
                            _ => None,
                        }
                    };
                    if let Ok(mut slot) = slot.lock() {
                        *slot = Some((generation, found));
                    }
                    let hwnd = HWND(hwnd as _);
                    let wp = WPARAM(generation as usize);
                    let _ = unsafe { PostMessageW(Some(hwnd), message, wp, LPARAM(0)) };
                }
            });
        Self { tx, answer }
    }

    pub fn request(&self, generation: u64, query: Query) {
        let _ = self.tx.send((generation, query));
    }
}

/// Tries the exact match first, then a search. `Err` means a request failed,
/// so "not found" cannot be trusted.
fn lookup(client: &Client, q: &Query) -> Result<Option<Arc<Lyrics>>, ()> {
    let mut failed = false;
    if q.duration > 0.0 {
        let mut path = format!(
            "/api/get?track_name={}&artist_name={}&duration={}",
            net::encode(&q.title),
            net::encode(&q.artist),
            q.duration.round()
        );
        if !q.album.is_empty() {
            path.push_str(&format!("&album_name={}", net::encode(&q.album)));
        }
        match get(client, &path) {
            Some((200, body)) => {
                let json = std::str::from_utf8(&body).ok().and_then(Json::parse);
                if let Some(json) = json {
                    if json.get("instrumental").is_some_and(Json::is_true) {
                        return Ok(None);
                    }
                    let lrc = json.get("syncedLyrics").and_then(Json::str);
                    if let Some(lyrics) = lrc.and_then(Lyrics::parse) {
                        return Ok(Some(Arc::new(lyrics)));
                    }
                }
            }
            Some((404, _)) => {}
            _ => failed = true,
        }
    }
    let (title, artist) = tidy(&q.title, &q.artist);
    let mut attempts = vec![(q.title.clone(), q.artist.clone())];
    if (title.as_str(), artist.as_str()) != (q.title.as_str(), q.artist.as_str()) {
        attempts.push((title, artist));
    }
    for (title, artist) in attempts {
        let path = format!(
            "/api/search?track_name={}&artist_name={}",
            net::encode(&title),
            net::encode(&artist)
        );
        match get(client, &path) {
            Some((200, body)) => {
                let json = std::str::from_utf8(&body).ok().and_then(Json::parse);
                if let Some(lyrics) = json.and_then(|j| best_match(&j, q.duration)) {
                    return Ok(Some(Arc::new(lyrics)));
                }
            }
            Some((404, _)) => {}
            _ => failed = true,
        }
    }
    if failed {
        Err(())
    } else {
        Ok(None)
    }
}

/// The synced result whose length is closest to the track's.
fn best_match(results: &Json, duration: f64) -> Option<Lyrics> {
    let mut best: Option<(f64, &str)> = None;
    for item in results.items() {
        let Some(lrc) = item.get("syncedLyrics").and_then(Json::str) else {
            continue;
        };
        let diff = match item.get("duration").and_then(Json::num) {
            Some(d) if duration > 0.0 => (d - duration).abs(),
            _ if duration > 0.0 => continue,
            _ => 0.0,
        };
        if diff <= DURATION_TOLERANCE && best.is_none_or(|(b, _)| diff < b) {
            best = Some((diff, lrc));
        }
    }
    Lyrics::parse(best?.1)
}

/// Undoes what video sites add to titles: "Artist - Song (Official Video)"
/// uploaded by "Artist - Topic" or "ArtistVEVO", and "feat." credits.
fn tidy(title: &str, artist: &str) -> (String, String) {
    const JUNK: [&str; 16] = [
        "official",
        "oficial",
        "video",
        "audio",
        "lyric",
        "lyrics",
        "letra",
        "visualizer",
        "remaster",
        "remastered",
        "hd",
        "4k",
        "mv",
        "feat",
        "ft",
        "featuring",
    ];
    let mut artist = artist
        .trim_end_matches(" - Topic")
        .trim_end_matches("VEVO")
        .trim()
        .to_string();
    let mut title = title.to_string();
    if let Some((left, right)) = title.split_once(" - ") {
        let (left, right) = (left.trim().to_string(), right.trim().to_string());
        if artist.is_empty() || squash(&left) == squash(&artist) {
            // "Artist - Song" uploaded by the artist's channel.
            artist = left;
            title = right;
        } else if words(&right).any(|w| SUFFIXES.contains(&w.as_str())) {
            // "Song - Remastered 2011", "Song - Radio Edit".
            title = left;
        }
    }
    // Drop "(...)" and "[...]" groups that hold junk or a featured artist.
    let mut out = String::new();
    let mut rest = title.as_str();
    while let Some(start) = rest.find(['(', '[']) {
        let close = if rest[start..].starts_with('(') {
            ')'
        } else {
            ']'
        };
        let Some(len) = rest[start..].find(close) else {
            break;
        };
        let drop = words(&rest[start + 1..start + len]).any(|w| JUNK.contains(&w.as_str()));
        out.push_str(&rest[..start]);
        if !drop {
            out.push_str(&rest[start..=start + len]);
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    let mut title = out.split_whitespace().collect::<Vec<_>>().join(" ");
    for marker in [" feat. ", " ft. ", " featuring "] {
        if let Some(i) = title.to_ascii_lowercase().find(marker) {
            title.truncate(i);
        }
    }
    // "A, B & C" → "A": LRCLIB lists the main artist first.
    if let Some(i) = artist.find([',', '&']) {
        artist.truncate(i);
    }
    (title.trim().to_string(), artist.trim().to_string())
}

const SUFFIXES: [&str; 7] = [
    "remaster",
    "remastered",
    "mono",
    "stereo",
    "version",
    "edit",
    "radio",
];

fn words(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

/// "The Weeknd" and "TheWeeknd" compare equal.
fn squash(s: &str) -> String {
    words(s).collect()
}

/// GET with one retry when LRCLIB says it is busy, as its answer asks.
fn get(client: &Client, path: &str) -> Option<(u32, Vec<u8>)> {
    match client.get(path) {
        Some((429 | 503, _)) => {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            client.get(path)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc() {
        let l = Lyrics::parse(
            "[ar:Someone]\n[offset:+500]\n[00:12.50]First\n[00:05.00][01:00.00] Twice \n[00:20.00]\n[00:30:25]<00:30.25>Word <00:31.00>sync",
        )
        .unwrap();
        let times: Vec<f64> = l.lines.iter().map(|l| l.time).collect();
        assert_eq!(times, [4.5, 12.0, 19.5, 29.75, 59.5]);
        assert_eq!(l.lines[0].text, "Twice");
        assert_eq!(l.lines[2].text, "");
        assert_eq!(l.lines[3].text, "Word sync");
        assert_eq!(l.index_at(0.0), None);
        assert_eq!(l.index_at(4.5), Some(0));
        assert_eq!(l.index_at(13.0), Some(1));
        assert_eq!(l.index_at(1000.0), Some(4));
        assert!(Lyrics::parse("plain text\nno tags").is_none());
    }

    #[test]
    fn picks_matching_duration() {
        let json = Json::parse(
            r#"[{"duration":248.0,"syncedLyrics":"[00:01.00]long"},
                {"duration":201.0,"syncedLyrics":null},
                {"duration":203.0,"syncedLyrics":"[00:01.00]close"},
                {"duration":202.0,"syncedLyrics":"[00:01.00]exact"}]"#,
        )
        .unwrap();
        let text = |d| best_match(&json, d).map(|l| l.lines[0].text.clone());
        assert_eq!(text(202.0).as_deref(), Some("exact"));
        assert_eq!(text(300.0), None);
        assert_eq!(text(0.0).as_deref(), Some("long"));
    }

    #[test]
    #[ignore = "talks to lrclib.net"]
    fn live_lookup() {
        let client = Client::new("Isla tests (https://github.com/sirtony27/isla)", HOST).unwrap();
        let query = |title: &str, artist: &str, album: &str, duration| Query {
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            duration,
        };
        let exact = lookup(
            &client,
            &query("One More Time", "Daft Punk", "Discovery", 320.0),
        );
        assert!(exact.unwrap().unwrap().lines.len() > 20);
        // No duration and a video-style title: found through the search.
        let video = query(
            "The Weeknd - Blinding Lights (Official Video)",
            "TheWeekndVEVO",
            "",
            0.0,
        );
        assert!(lookup(&client, &video).unwrap().is_some());
        let missing = query("Zzqx nonexistent song", "Nobody Qq", "", 0.0);
        assert!(lookup(&client, &missing).unwrap().is_none());
    }

    #[test]
    fn tidies_video_titles() {
        let t = |a: &str, b: &str| tidy(a, b);
        assert_eq!(
            t("Daft Punk - One More Time (Official Video)", "Daft Punk"),
            ("One More Time".into(), "Daft Punk".into())
        );
        assert_eq!(
            t("Song [HD] (feat. B)", "ArtistVEVO"),
            ("Song".into(), "Artist".into())
        );
        assert_eq!(
            t("Bohemian Rhapsody - Remastered 2011", "Queen"),
            ("Bohemian Rhapsody".into(), "Queen".into())
        );
        assert_eq!(
            t(
                "The Weeknd - Blinding Lights (Official Video)",
                "TheWeekndVEVO"
            ),
            ("Blinding Lights".into(), "The Weeknd".into())
        );
        assert_eq!(
            t("Song (Live) ft. X", "A, B & C"),
            ("Song (Live)".into(), "A".into())
        );
    }
}
