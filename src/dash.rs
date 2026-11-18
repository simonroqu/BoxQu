//! Lokaler Zwischenserver für MovieBox-DASH (Serien): Der DASH-Server bremst jede Verbindung auf ≈140 KB/s, mpv
//! lädt die 5-s-Stücke aber nacheinander über eine – 1080p kam so nur mit ≈0,3× Echtzeit an (gemessen 2026-11-18).
//! Hier holen wir jedes Stück in Teilen (Range) gleichzeitig und die nächsten `AHEAD` schon vorab; mpv lädt von 127.0.0.1.
//! Jede Bildhöhe bekommt ein eigenes Manifest (nur diese Stufe), damit das Qualitätsmenü des Players greift.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::OnceCell;

/// So viele Stücke je Spur vorladen (Video und Audio getrennt).
const AHEAD: u32 = 4;
/// Teilgröße: ein 1080p-Stück (bis ≈2,6 MB) kommt so über ≈10 Verbindungen gleichzeitig (gemessen: Start in 3 s,
/// danach ≈12× Echtzeit; vorher 0,3×).
const PART: u64 = 256 * 1024;
/// Der erste Teil allein (er verrät erst die Gesamtgröße), daher klein: ≈0,5 s bei der Drosselung.
const FIRST: u64 = 64 * 1024;

type Body = Result<Arc<Vec<u8>>, String>;

struct Upstream {
    /// Wechselt mit jedem neuen Stream; Anfragen an einen alten gehen ins Leere.
    id: u32,
    /// Ordner des Manifests auf dem Server (mit „/“ am Ende).
    base: String,
    headers: Vec<(String, String)>,
    mpd: String,
    cache: HashMap<String, Arc<OnceCell<Body>>>,
}

static CURRENT: Mutex<Option<Upstream>> = Mutex::new(None);
static GEN: AtomicU32 = AtomicU32::new(0);
static PORT: OnceLock<Option<u16>> = OnceLock::new();
// Nur HTTP/1.1: über HTTP/2 liefen alle Teile durch eine (gedrosselte) Verbindung. Ohne Gesamt-Zeitlimit.
static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .http1_only()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .build()
        .expect("HTTP-Client")
});

/// Manifest holen und über den Zwischenserver anbieten: Stufen (Höhe, lokale Adresse), beste zuerst.
/// `None`, wenn das nicht geht (dann spielt mpv den DASH-Stream direkt wie bisher).
pub async fn serve(url: &str, headers: Vec<(String, String)>) -> Option<Vec<(u32, String)>> {
    let port = (*PORT.get_or_init(listen))?;
    let mpd = fetch(url, &headers).await.ok()?;
    let mpd = String::from_utf8(mpd.to_vec()).ok()?;
    let mut heights = video_heights(&mpd);
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    if heights.is_empty() {
        return None;
    }
    let id = GEN.fetch_add(1, Ordering::Relaxed) + 1;
    let base = url[..url.rfind('/')? + 1].to_string();
    let reps: Vec<String> = rep_ids(&mpd).into_iter().map(String::from).collect();
    let mut up = Upstream { id, base, headers, mpd, cache: HashMap::new() };
    // Aufwärmen (wie MovieBox-Tui): Was ffmpeg beim Öffnen immer zuerst holt – Init und erstes Stück jeder Spur,
    // auch beim Fortsetzen –, schon jetzt für alle Stufen (welche der Player nimmt, wissen wir hier nicht). Läuft,
    // während `source::stream` noch die Untertitel lädt. Namen wie im `SegmentTemplate` der MovieBox-Manifeste.
    for rep in reps {
        preload(&mut up, format!("init-stream{rep}.m4s"));
        preload(&mut up, format!("chunk-stream{rep}-00001.m4s"));
    }
    *CURRENT.lock().unwrap() = Some(up);
    Some(heights.into_iter().map(|h| (h, format!("http://127.0.0.1:{port}/{id}/{h}/index.mpd"))).collect())
}

fn listen() -> Option<u16> {
    let std = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    std.set_nonblocking(true).ok()?;
    let port = std.local_addr().ok()?.port();
    let listener = tokio::net::TcpListener::from_std(std).ok()?;
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(handle(sock));
        }
    });
    Some(port)
}

/// Datei holen, in Teilen zu `PART` gleichzeitig (jede Verbindung ist gedrosselt). Kann der Server keine Teile
/// (Antwort 200 statt 206), kommt sie schon mit der ersten Anfrage ganz.
async fn fetch(url: &str, headers: &[(String, String)]) -> Body {
    let (mut body, total) = get(url, headers, 0, FIRST).await?;
    let Some(total) = total else { return Ok(Arc::new(body)) };
    let parts: Vec<_> = (FIRST..total)
        .step_by(PART as usize)
        .map(|from| {
            let (url, headers) = (url.to_string(), headers.to_vec());
            tokio::spawn(async move { get(&url, &headers, from, PART).await })
        })
        .collect();
    for part in parts {
        body.extend(part.await.map_err(|e| e.to_string())??.0);
    }
    Ok(Arc::new(body))
}

/// Ein Teil ab `from`; dazu die Gesamtgröße, wenn der Server in Teilen liefert. Bei Fehlern (der Server lehnt
/// gelegentlich kurz ab) bis zu dreimal, sonst fehlt mpv gleich ein ganzes Stück.
async fn get(url: &str, headers: &[(String, String)], from: u64, len: u64) -> Result<(Vec<u8>, Option<u64>), String> {
    let mut tries = 0;
    loop {
        match get_once(url, headers, from, len).await {
            Err(_) if tries < 2 => tries += 1,
            r => return r,
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn get_once(url: &str, headers: &[(String, String)], from: u64, len: u64) -> Result<(Vec<u8>, Option<u64>), String> {
    let mut req = HTTP.get(url).header("Range", format!("bytes={from}-{}", from + len - 1));
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
    let total = (resp.status() == reqwest::StatusCode::PARTIAL_CONTENT)
        .then(|| resp.headers().get("content-range")?.to_str().ok()?.rsplit('/').next()?.parse().ok())
        .flatten();
    Ok((resp.bytes().await.map_err(|e| e.to_string())?.to_vec(), total))
}

/// Eine Anfrage von mpv: `GET /<id>/<höhe>/<datei>`, Antwort immer ganz (200) und Verbindung zu.
async fn handle(mut sock: TcpStream) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16384 {
        match sock.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let path = head.split_whitespace().nth(1).unwrap_or_default().to_string();
    let (status, body) = match answer(&path).await {
        Some(b) => ("200 OK", b),
        None => ("404 Not Found", Arc::new(Vec::new())),
    };
    let header = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    let _ = sock.write_all(header.as_bytes()).await;
    let _ = sock.write_all(&body).await;
}

async fn answer(path: &str) -> Option<Arc<Vec<u8>>> {
    let mut parts = path.trim_start_matches('/').splitn(3, '/');
    let (id, height, file) = (parts.next()?.parse::<u32>().ok()?, parts.next()?.parse::<u32>().ok()?, parts.next()?);
    if file.contains('/') || file.contains("..") {
        return None;
    }
    let (cell, base, headers) = {
        let mut cur = CURRENT.lock().unwrap();
        let up = cur.as_mut().filter(|u| u.id == id)?;
        if file == "index.mpd" {
            return Some(Arc::new(only_height(&up.mpd, height).into_bytes()));
        }
        // Nachfolgende Stücke anstoßen; Vorgeladenes außerhalb des Fensters (nach Springen) verwerfen.
        if let Some((prefix, n)) = segment(file) {
            up.cache.retain(|k, _| segment(k).is_none_or(|(p, m)| p != prefix || (n..=n + AHEAD).contains(&m)));
            for m in n + 1..=n + AHEAD {
                preload(up, format!("{prefix}-{m:05}.m4s"));
            }
        }
        (up.cache.entry(file.to_string()).or_default().clone(), up.base.clone(), up.headers.clone())
    };
    let body = cell.get_or_init(|| async { fetch(&format!("{base}{file}"), &headers).await }).await.clone();
    // Fehlgeschlagen: raus aus dem Speicher, ein erneuter Versuch lädt neu. Geholtes bleibt, bis das Fenster
    // weiterzieht (ffmpeg fragt das erste Stück beim Öffnen zweimal an).
    if body.is_err()
        && let Some(up) = CURRENT.lock().unwrap().as_mut().filter(|u| u.id == id)
    {
        up.cache.remove(file);
    }
    body.ok()
}

/// Im Hintergrund holen, falls noch nicht im Speicher.
fn preload(up: &mut Upstream, name: String) {
    if up.cache.contains_key(&name) {
        return;
    }
    let cell: Arc<OnceCell<Body>> = Arc::default();
    up.cache.insert(name.clone(), cell.clone());
    let (url, headers) = (format!("{}{name}", up.base), up.headers.clone());
    tokio::spawn(async move {
        cell.get_or_init(|| async { fetch(&url, &headers).await }).await;
    });
}

/// „chunk-stream1-00042.m4s“ → („chunk-stream1“, 42)
fn segment(file: &str) -> Option<(&str, u32)> {
    let (prefix, num) = file.strip_suffix(".m4s")?.rsplit_once('-')?;
    Some((prefix, num.parse().ok()?))
}

/// Video-`Representation`s als (Anfang, Ende, Höhe) im Manifest.
fn video_reps(mpd: &str) -> Vec<(usize, usize, u32)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = mpd[from..].find("<Representation") {
        let start = from + i;
        let Some(len) = mpd[start..].find("</Representation>") else { break };
        let end = start + len + "</Representation>".len();
        let tag = &mpd[start..start + mpd[start..].find('>').unwrap_or(0)];
        if tag.contains("mimeType=\"video")
            && let Some(h) = tag.split(" height=\"").nth(1).and_then(|s| s.split('"').next()?.parse().ok())
        {
            out.push((start, end, h));
        }
        from = end;
    }
    out
}

/// IDs aller `Representation`s (Video und Audio).
fn rep_ids(mpd: &str) -> Vec<&str> {
    mpd.split("<Representation id=\"").skip(1).filter_map(|s| s.split('"').next()).collect()
}

fn video_heights(mpd: &str) -> Vec<u32> {
    video_reps(mpd).into_iter().map(|r| r.2).collect()
}

/// Manifest nur mit den Video-Stufen dieser Höhe (Audio bleibt).
fn only_height(mpd: &str, height: u32) -> String {
    let mut out = mpd.to_string();
    for (start, end, h) in video_reps(mpd).into_iter().rev() {
        if h != height {
            out.replace_range(start..end, "");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MPD: &str = r#"<MPD><Period><AdaptationSet contentType="video">
<Representation id="0" mimeType="video/mp4" width="1920" height="1080"><SegmentTemplate media="a"></SegmentTemplate></Representation>
<Representation id="1" mimeType="video/mp4" width="1280" height="720"><SegmentTemplate media="a"></SegmentTemplate></Representation>
</AdaptationSet><AdaptationSet contentType="audio">
<Representation id="3" mimeType="audio/mp4"><SegmentTemplate media="a"></SegmentTemplate></Representation>
</AdaptationSet></Period></MPD>"#;

    #[test]
    fn manifest() {
        assert_eq!(video_heights(MPD), vec![1080, 720]);
        assert_eq!(rep_ids(MPD), vec!["0", "1", "3"]);
        let m = only_height(MPD, 720);
        assert!(m.contains(r#"id="1""#) && m.contains(r#"id="3""#) && !m.contains(r#"id="0""#));
        assert_eq!(segment("chunk-stream1-00042.m4s"), Some(("chunk-stream1", 42)));
        assert_eq!(segment("init-stream1.m4s"), None);
    }
}
