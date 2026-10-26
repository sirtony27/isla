//! Synced lyrics from LRCLIB (https://lrclib.net), a free, open lyrics database.
//!
//! Off by default. When on, the title, artist, album and duration of each new
//! track are sent to lrclib.net, and nothing else. Requests run on their own
//! thread, which sleeps on a channel between tracks. LRCLIB is often busy, so
//! a failed lookup is tried again while the same track plays.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::json::Json;
use crate::net::{self, Client};

const HOST: &str = "lrclib.net";
/// LRCLIB returns versions of a song whose length differs by more than this
/// (radio edit, live...). Their timings would not match what is playing.
const DURATION_TOLERANCE: f64 = 3.0;
const CACHE_SIZE: usize = 24;
/// Seconds to wait before each new try while LRCLIB is busy or unreachable:
/// about 40 s in all, well within a song. A track change ends the wait.
const RETRY_DELAYS: [u64; 4] = [2, 5, 10, 20];

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

type Request = (u64, Query);
type Found = Option<Arc<Lyrics>>;

pub struct Fetcher {
    tx: Sender<Request>,
    pub answer: Arc<Mutex<Option<Answer>>>,
}

impl Fetcher {
    /// Starts the worker; it posts `message` (WPARAM = generation) when it has an
    /// answer, and once more if a retry finds the lyrics later.
    /// Dropping the fetcher ends the thread.
    pub fn start(hwnd: HWND, message: u32) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
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
                let delays = RETRY_DELAYS.map(Duration::from_secs);
                let mut cache: Vec<(String, Found)> = Vec::new();
                let mut next = None;
                while let Some(first) = next.take().or_else(|| rx.recv().ok()) {
                    // Only the newest track matters when the user skips quickly.
                    let (generation, query) = rx.try_iter().last().unwrap_or(first);
                    let publish = |found: Found| {
                        if let Ok(mut slot) = slot.lock() {
                            *slot = Some((generation, found));
                        }
                        let hwnd = HWND(hwnd as _);
                        let wp = WPARAM(generation as usize);
                        let _ = unsafe { PostMessageW(Some(hwnd), message, wp, LPARAM(0)) };
                    };
                    let key = query.key();
                    let found = if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
                        let entry = cache.remove(i);
                        let found = entry.1.clone();
                        cache.push(entry);
                        found
                    } else {
                        let mut lookup = Lookup::new(&query);
                        let outcome = with_retries(
                            &rx,
                            &delays,
                            // A connection per try, closed right after: nothing stays
                            // open between songs or while waiting to try again.
                            || match Client::new(&agent, HOST) {
                                Some(client) => lookup.attempt(|path| client.get(path)),
                                None => Err(()),
                            },
                            // After two failed tries, stop holding the lyrics layout
                            // of the previous track while waiting.
                            |tries| {
                                if tries == 2 {
                                    publish(None);
                                }
                            },
                        );
                        match outcome {
                            Outcome::Done(Ok(found)) => {
                                cache.push((key, found.clone()));
                                if cache.len() > CACHE_SIZE {
                                    cache.remove(0);
                                }
                                found
                            }
                            // Offline or busy the whole time: not cached, so the track
                            // is looked up again the next time it plays.
                            Outcome::Done(Err(())) => None,
                            Outcome::Superseded(newer) => {
                                next = Some(newer);
                                continue;
                            }
                            Outcome::Closed => return,
                        }
                    };
                    publish(found);
                }
            });
        Self { tx, answer }
    }

    pub fn request(&self, generation: u64, query: Query) {
        let _ = self.tx.send((generation, query));
    }
}

/// How a lookup with retries ended.
enum Outcome {
    /// The lookup's answer, or `Err` if every try failed.
    Done(Result<Found, ()>),
    /// A newer track arrived while waiting to try again.
    Superseded(Request),
    /// The fetcher was dropped.
    Closed,
}

/// Runs `attempt` until it gives a real answer, waiting longer before each new
/// try. The wait is on the request channel, so a newer track ends it at once.
/// `waiting` gets the number of failed tries before each wait.
fn with_retries(
    rx: &Receiver<Request>,
    delays: &[Duration],
    mut attempt: impl FnMut() -> Result<Found, ()>,
    mut waiting: impl FnMut(usize),
) -> Outcome {
    let mut result = attempt();
    for (tries, &delay) in (1..).zip(delays) {
        if result.is_ok() {
            break;
        }
        waiting(tries);
        match rx.recv_timeout(delay) {
            Ok(newer) => return Outcome::Superseded(newer),
            Err(RecvTimeoutError::Timeout) => result = attempt(),
            Err(RecvTimeoutError::Disconnected) => return Outcome::Closed,
        }
    }
    Outcome::Done(result)
}

/// The requests for one track, in order: the exact match, then a search with
/// the title as the player gives it and one with it tidied. A request that got
/// an answer, found or not, is not sent again when the lookup is retried.
struct Lookup {
    /// Path, and whether it is `/api/get` (one record) rather than a search (a list).
    pending: Vec<(String, bool)>,
    duration: f64,
}

impl Lookup {
    fn new(q: &Query) -> Self {
        let mut pending = Vec::new();
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
            pending.push((path, true));
        }
        let (title, artist) = tidy(&q.title, &q.artist);
        let mut searches = vec![(q.title.clone(), q.artist.clone())];
        if (title.as_str(), artist.as_str()) != (q.title.as_str(), q.artist.as_str()) {
            searches.push((title, artist));
        }
        for (title, artist) in searches {
            let path = format!(
                "/api/search?track_name={}&artist_name={}",
                net::encode(&title),
                net::encode(&artist)
            );
            pending.push((path, false));
        }
        Self {
            pending,
            duration: q.duration,
        }
    }

    /// Sends the requests still pending, until one finds the lyrics. `Err` means
    /// LRCLIB was busy or unreachable for some of them, so "not found" cannot
    /// be trusted yet.
    fn attempt(
        &mut self,
        mut get: impl FnMut(&str) -> Option<(u32, Vec<u8>)>,
    ) -> Result<Found, ()> {
        let mut i = 0;
        while i < self.pending.len() {
            let (path, exact) = &self.pending[i];
            let exact = *exact;
            match get(path) {
                Some((200, body)) => {
                    let json = std::str::from_utf8(&body).ok().and_then(Json::parse);
                    if let Some(json) = json {
                        if exact && json.get("instrumental").is_some_and(Json::is_true) {
                            return Ok(None);
                        }
                        let lyrics = if exact {
                            let lrc = json.get("syncedLyrics").and_then(Json::str);
                            lrc.and_then(Lyrics::parse)
                        } else {
                            best_match(&json, self.duration)
                        };
                        if let Some(lyrics) = lyrics {
                            return Ok(Some(Arc::new(lyrics)));
                        }
                    }
                    self.pending.remove(i);
                }
                // Busy or unreachable: kept for the next try.
                Some((429 | 500..=599, _)) | None => i += 1,
                // Not found, or a refusal that trying again would not change.
                Some(_) => {
                    self.pending.remove(i);
                }
            }
        }
        if self.pending.is_empty() {
            Ok(None)
        } else {
            Err(())
        }
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

    fn query(title: &str, artist: &str, album: &str, duration: f64) -> Query {
        Query {
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            duration,
        }
    }

    #[test]
    fn retries_only_what_failed() {
        let q = query(
            "Daft Punk - One More Time (Official Video)",
            "DaftPunkVEVO",
            "",
            320.0,
        );
        let mut lookup = Lookup::new(&q);
        let mut sent = Vec::new();
        // Busy, nothing found, unreachable.
        let mut replies = [Some((503, vec![])), Some((200, b"[]".to_vec())), None].into_iter();
        let first = lookup.attempt(|path| {
            sent.push(path.to_string());
            replies.next().unwrap()
        });
        assert!(first.is_err());
        assert_eq!(sent.len(), 3);
        // The second try skips the search that already answered.
        sent.clear();
        let found = br#"[{"duration":321.0,"syncedLyrics":"[00:01.00]One more time"}]"#;
        let mut replies = [Some((404, vec![])), Some((200, found.to_vec()))].into_iter();
        let second = lookup.attempt(|path| {
            sent.push(path.to_string());
            replies.next().unwrap()
        });
        assert_eq!(second.unwrap().unwrap().lines[0].text, "One more time");
        assert!(sent[0].starts_with("/api/get?"));
        assert!(sent[1].ends_with("track_name=One%20More%20Time&artist_name=Daft%20Punk"));
        assert_eq!(sent.len(), 2);
    }

    #[test]
    fn answers_are_final() {
        let q = query("Song", "Artist", "", 200.0);
        // Not found anywhere, and a refusal a retry would not change.
        let none = Lookup::new(&q).attempt(|path| match path.starts_with("/api/get") {
            true => Some((404, vec![])),
            false => Some((200, b"[]".to_vec())),
        });
        assert!(matches!(none, Ok(None)));
        assert!(matches!(
            Lookup::new(&q).attempt(|_| Some((400, vec![]))),
            Ok(None)
        ));
        // Instrumental: no lyrics, and no search after it.
        let mut sent = 0;
        let instrumental = Lookup::new(&q).attempt(|_| {
            sent += 1;
            Some((200, br#"{"instrumental":true}"#.to_vec()))
        });
        assert!(matches!(instrumental, Ok(None)));
        assert_eq!(sent, 1);
    }

    #[test]
    fn retries_until_answered_or_superseded() {
        let (tx, rx) = mpsc::channel::<Request>();
        let delays = [Duration::from_millis(1); 3];
        let lyrics = Arc::new(Lyrics::parse("[00:01.00]x").unwrap());
        // Fails twice, then finds the lyrics.
        let (mut tries, mut waits) = (0, Vec::new());
        let outcome = with_retries(
            &rx,
            &delays,
            || {
                tries += 1;
                if tries < 3 {
                    Err(())
                } else {
                    Ok(Some(lyrics.clone()))
                }
            },
            |n| waits.push(n),
        );
        assert!(matches!(outcome, Outcome::Done(Ok(Some(_)))));
        assert_eq!((tries, waits), (3, vec![1, 2]));
        // Never answers: one try, then one per delay.
        let mut tries = 0;
        let outcome = with_retries(
            &rx,
            &delays,
            || {
                tries += 1;
                Err(())
            },
            |_| {},
        );
        assert!(matches!(outcome, Outcome::Done(Err(()))));
        assert_eq!(tries, 4);
        // A newer track ends the wait.
        tx.send((7, query("Next", "Artist", "", 0.0))).unwrap();
        let mut tries = 0;
        let outcome = with_retries(
            &rx,
            &delays,
            || {
                tries += 1;
                Err(())
            },
            |_| {},
        );
        assert!(matches!(outcome, Outcome::Superseded((7, _))));
        assert_eq!(tries, 1);
        // The fetcher is gone.
        drop(tx);
        let outcome = with_retries(&rx, &delays, || Err(()), |_| {});
        assert!(matches!(outcome, Outcome::Closed));
    }

    #[test]
    #[ignore = "talks to lrclib.net"]
    fn live_lookup() {
        let client = Client::new("Isla tests (https://github.com/sirtony27/isla)", HOST).unwrap();
        // LRCLIB is often busy: spaced tries, as the fetcher does.
        let find = |q: &Query| -> Found {
            let mut lookup = Lookup::new(q);
            for _ in 0..5 {
                if let Ok(found) = lookup.attempt(|path| client.get(path)) {
                    return found;
                }
                std::thread::sleep(Duration::from_secs(3));
            }
            panic!("lrclib.net stayed busy");
        };
        let exact = find(&query("One More Time", "Daft Punk", "Discovery", 320.0));
        assert!(exact.unwrap().lines.len() > 20);
        // No duration and a video-style title: found through the search.
        let video = query(
            "The Weeknd - Blinding Lights (Official Video)",
            "TheWeekndVEVO",
            "",
            0.0,
        );
        assert!(find(&video).is_some());
        assert!(find(&query("Zzqx nonexistent song", "Nobody Qq", "", 0.0)).is_none());
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
