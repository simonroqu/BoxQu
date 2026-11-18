//! Einzige Stelle, die Inhalte holt: MovieBox-Endpunkte (über `moviebox.rs`) und deren JSON → BoxQu-Typen.
//! Abläufe wie in MovieBox-Tui (`providers/moviebox/{mod,adapt}.rs`), dort nachsehen, wenn MovieBox etwas ändert.
//!
//! IDs: Titel = MovieBox-`subjectId`. Episode = `<subjectId>/<staffel>/<folge>` (Film: `/0/0`).

use crate::moviebox;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::sync::Semaphore;

pub const USER_AGENT: &str = concat!("BoxQu/", env!("CARGO_PKG_VERSION"));
/// Gleichzeitige Poster-Downloads.
const POSTER_PARALLEL: usize = 6;
/// Startseiten-Bereiche: MovieBox-Homepage-Tabs (`url` = Tab-ID). „2“ = Trending, wie in der TUI.
const SECTIONS: [(&str, &str); 1] = [("2", "Trending")];
/// Höchstens so viele Startseiten-Seiten nachladen („alle Titel“ eines Bereichs).
const MAX_PAGES: usize = 5;
const API: &str = "/wefeed-mobile-bff/subject-api";

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder().user_agent(USER_AGENT).timeout(Duration::from_secs(20)).build().expect("HTTP-Client")
});
static POSTER_SLOTS: Semaphore = Semaphore::const_new(POSTER_PARALLEL);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Title {
    pub id: String,
    pub title: String,
    pub poster: Option<String>,
    /// Jahr o. Ä. für die Zeile unter dem Poster.
    pub kind: Option<String>,
    /// Laufzeit, z. B. „1:32:00“.
    pub duration: Option<String>,
}

pub struct Details {
    pub item: Title,
    pub description: String,
    pub info: Vec<String>,
    pub genres: Vec<String>,
    /// Sprachfassungen (bei MovieBox je ein eigener Titel); leer oder eine = kein Umschalter.
    pub languages: Vec<Language>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Language {
    /// `lanCode` („en“, „hi“, „esla“ …); die Original-Fassung wird über `original` erkannt.
    pub code: String,
    pub name: String,
    /// Titel-ID dieser Fassung.
    pub id: String,
    pub original: bool,
}

/// Lesbarer Name einer Fassung: „Hindi dub“ → „Hindi“, Sonderfälle der Codes ausgeschrieben.
fn language_name(code: &str, name: &str, original: bool) -> String {
    if original {
        return "Original".into();
    }
    match code {
        "esla" => "Spanish (Latin America)".into(),
        "ptbr" => "Portuguese (Brazil)".into(),
        _ => {
            let n = name.trim().trim_end_matches(" dub").trim();
            let n = if n.is_empty() { code } else { n };
            let mut c = n.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub id: String,
    pub number: u32,
    pub title: String,
}

#[derive(Clone, Debug, Default)]
pub struct Stream {
    /// Qualitätsstufen (Bildhöhe, Adresse), beste zuerst. Höhe 0 = unbekannt.
    pub variants: Vec<(u32, String)>,
    /// HTTP-Header für mpv (User-Agent, Referer, Cookie …), gelten für alle Stufen.
    pub headers: Vec<(String, String)>,
    /// Untertitel als lokale Dateien („…/English.srt“), bevorzugte Sprache zuerst.
    pub subtitles: Vec<String>,
    /// Immer `None` (ani-skip gibt es nur für Animes); Platz für spätere Intro-Zeiten.
    pub mal_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub url: String,
    pub name: String,
}

/// Was ein eingefügter Link war.
#[allow(dead_code)]
pub enum Link {
    Source,
    Title(Title),
    Video(Title),
}

// ---------- JSON-Hilfen ----------

/// Zahl oder Text als Text (IDs kommen mal so, mal so).
fn text(v: &Value) -> Option<String> {
    v.as_i64().map(|n| n.to_string()).or_else(|| v.as_str().map(String::from)).filter(|s| !s.is_empty())
}

fn num(v: &Value) -> Option<usize> {
    v.as_u64().map(|n| n as usize).or_else(|| v.as_str()?.parse().ok())
}

/// Erste vierstellige Zahl („2024-05-01“ → „2024“).
fn year(s: &str) -> Option<String> {
    let d: Vec<char> = s.chars().collect();
    d.windows(4).find(|w| w.iter().all(char::is_ascii_digit)).map(|w| w.iter().collect())
}

/// Titel ohne MovieBox-Zusätze: „Breaking Bad [Hindi] S5“ → „Breaking Bad“, „Ramayana [Hindi] [CAM]“ → „Ramayana“.
/// Die Sprache steht ohnehin auf der Titel-Seite (Tabs), die Staffeln in den Episoden.
fn clean_title(raw: &str) -> String {
    let t = raw.trim();
    let t = t.find(" [").map_or(t, |i| &t[..i]);
    let t = match t.rsplit_once(" S") {
        Some((head, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '-') => head,
        _ => t,
    };
    if t.trim().is_empty() { raw.trim().to_string() } else { t.trim().to_string() }
}

/// Ein `subject` aus Suche/Startseite/Details als Titel.
fn subject(s: &Value) -> Option<Title> {
    let id = text(&s["subjectId"]).or_else(|| text(&s["id"]))?;
    let y = ["releaseDate", "year", "releaseInfo"].iter().find_map(|k| s[*k].as_str()).and_then(year);
    let series = s["subjectType"].as_i64().or(s["stype"].as_i64()) == Some(2);
    let kind: Vec<String> = y.into_iter().chain(series.then(|| "Series".to_string())).collect();
    Some(Title {
        id,
        title: clean_title(s["title"].as_str().or(s["name"].as_str()).unwrap_or("Unknown")),
        poster: [&s["cover"]["url"], &s["coverUrl"], &s["poster"], &s["pic"]].iter().find_map(|v| v.as_str()).map(String::from),
        kind: (!kind.is_empty()).then(|| kind.join(" · ")),
        duration: None,
    })
}

/// Ohne Doppelte: gleiche ID oder gleicher Titel mit gleichem Jahr (andere Sprachfassung desselben Films).
fn push_unique(out: &mut Vec<Title>, t: Title) {
    if !out.iter().any(|o| o.id == t.id || (o.title == t.title && o.kind == t.kind)) {
        out.push(t);
    }
}

// ---------- Startseite, Suche ----------

/// Feste Bereiche der Startseite.
pub fn sources() -> Vec<Source> {
    SECTIONS.iter().map(|(u, n)| Source { url: (*u).into(), name: (*n).into() }).collect()
}

/// Bereiche sind fest, nichts zu entfernen.
pub fn remove_source(_url: &str) {}

/// Links werden (noch) nicht erkannt; Suche nach Namen geht.
pub async fn open_link(_url: String) -> Result<Link, String> {
    Err("Links aren't supported, search by name instead".into())
}

/// Titel einer Startseiten-Antwort: Banner, `customData` und `subjects` jeder Gruppe.
fn homepage_titles(v: &Value) -> Vec<Title> {
    let mut out = Vec::new();
    let groups = v["items"].as_array().or(v.as_array()).cloned().unwrap_or_default();
    for g in &groups {
        let banners = g["banner"]["banners"].as_array().into_iter().flatten().map(|b| &b["subject"]);
        let custom = g["customData"]["items"].as_array().into_iter().flatten().map(|c| &c["subject"]);
        let subjects = g["subjects"].as_array().into_iter().flatten();
        for t in banners.chain(custom).chain(subjects).filter_map(subject) {
            push_unique(&mut out, t);
        }
    }
    out
}

/// Die ersten `limit` Titel eines Startseiten-Tabs.
pub async fn titles(src: Source, limit: usize) -> Result<Vec<Title>, String> {
    let mut out: Vec<Title> = Vec::new();
    for page in 1..=MAX_PAGES {
        let v = moviebox::get(&format!("/wefeed-mobile-bff/tab-operating?page={page}&tabId={}&version=", src.url)).await?;
        let before = out.len();
        for t in homepage_titles(&v) {
            push_unique(&mut out, t);
        }
        if out.len() >= limit || out.len() == before {
            break;
        }
    }
    out.truncate(limit);
    Ok(out)
}

fn search_titles(v: &Value) -> Vec<Title> {
    let list = v["results"][0]["subjects"].as_array().or(v["list"].as_array());
    let mut out = Vec::new();
    for t in list.into_iter().flatten().filter_map(subject) {
        push_unique(&mut out, t);
    }
    out
}

pub async fn search(query: String) -> Result<Vec<Title>, String> {
    let v = moviebox::post(&format!("{API}/search/v2"), &json!({"keyword": query, "page": 1, "perPage": 15, "subjectType": 0})).await?;
    Ok(search_titles(&v))
}

// ---------- Titel-Seite, Episoden ----------

/// Titel-Daten; bei Serien zusätzlich die Staffeln (`seasons`).
async fn subject_json(id: &str) -> Result<Value, String> {
    let mut v = moviebox::get(&format!("{API}/get?subjectId={id}")).await?;
    if v.get("subject").is_some() {
        v = v["subject"].take();
    }
    if v["subjectType"].as_i64().or(v["stype"].as_i64()) == Some(2) {
        if let Ok(seasons) = moviebox::get(&format!("{API}/season-info?subjectId={id}")).await {
            v["seasons"] = seasons;
        }
    }
    Ok(v)
}

fn parse_details(v: &Value) -> Result<Details, String> {
    let mut item = subject(v).ok_or("Title not found")?;
    item.duration = match &v["duration"] {
        Value::Number(n) => n.as_u64().filter(|n| *n > 0).map(|n| format!("{}m", n / 60)),
        d => d.as_str().map(str::trim).filter(|s| !s.is_empty() && *s != "0" && *s != "0m").map(String::from),
    };
    let rating = v["imdbRatingValue"].as_f64().map(|r| format!("{r:.1}")).or_else(|| v["imdbRatingValue"].as_str().map(String::from));
    let info = [rating.map(|r| format!("IMDb {r}")), text(&v["director"]), text(&v["stars"]), text(&v["audios"])].into_iter().flatten().collect();
    let genres = match &v["genre"] {
        Value::Array(a) => a.iter().filter_map(|g| g.as_str().map(String::from)).collect(),
        // MovieBox schickt Genres meist als „Action,Drama“.
        Value::String(s) => s.split(',').map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect(),
        _ => vec![],
    };
    let description = v["description"].as_str().or(v["intro"].as_str()).unwrap_or_default().to_string();
    let languages = v["dubs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let (code, original) = (d["lanCode"].as_str().unwrap_or_default(), d["original"].as_bool().unwrap_or(false));
            Some(Language { code: code.into(), name: language_name(code, d["lanName"].as_str().unwrap_or_default(), original), id: text(&d["subjectId"])?, original })
        })
        .collect();
    Ok(Details { item, description, info, genres, languages })
}

/// (Staffel, Folge) aller Folgen: `episodeNumbers` oder 1…`maxEp` je Staffel.
fn parse_episodes(v: &Value) -> Vec<(usize, usize)> {
    let seasons = v["seasons"]["seasons"].as_array().or(v["seasons"].as_array()).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for s in &seasons {
        let se = num(&s["se"]).unwrap_or(1);
        match s["episodeNumbers"].as_array() {
            Some(eps) => out.extend(eps.iter().filter_map(num).map(|e| (se, e))),
            None => out.extend((1..=num(&s["maxEp"]).unwrap_or(0)).map(|e| (se, e))),
        }
    }
    out
}

pub async fn details(id: String) -> Result<Details, String> {
    parse_details(&subject_json(&id).await?)
}

/// Alle Folgen aller Staffeln fortlaufend nummeriert (1, 2, 3 …); Film = eine Episode ohne Titel.
pub async fn episodes(id: String) -> Result<Vec<Episode>, String> {
    let eps = parse_episodes(&subject_json(&id).await?);
    if eps.is_empty() {
        return Ok(vec![Episode { id: format!("{id}/0/0"), number: 1, title: String::new() }]);
    }
    Ok(eps
        .into_iter()
        .enumerate()
        .map(|(i, (s, e))| Episode { id: format!("{id}/{s}/{e}"), number: i as u32 + 1, title: format!("S{s} E{e}") })
        .collect())
}

// ---------- Stream ----------

fn parse_episode(episode_id: &str) -> Option<(&str, usize, usize)> {
    let mut it = episode_id.rsplitn(3, '/');
    let (e, s, id) = (it.next()?, it.next()?, it.next()?);
    Some((id, s.parse().ok()?, e.parse().ok()?))
}

/// Hinweis-Clip („bitte App aktualisieren“) statt des Films.
fn is_placeholder(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    ["1c7de0bd3393702d9191801f15f88f8d", "9a0461bc39da389663bf3dbb17091d3f", "b164fbfb4347792950bdfbfb563d39d9", "/notice.mp4"].iter().any(|p| l.contains(p))
        || (l.contains("macdn.aoneroom.com") && l.contains("/other/"))
}

/// Bildhöhe: Zahl, „1080p“, „4K“; sonst aus dem Text („…_720P.mp4“), sonst 1080 wie in der TUI.
fn height(v: &Value, link: &str) -> u32 {
    if let Some(n) = v.as_u64() {
        return n as u32;
    }
    let s = v.as_str().unwrap_or_default().trim();
    if s.eq_ignore_ascii_case("4k") || s.eq_ignore_ascii_case("uhd") {
        return 2160;
    }
    if let Ok(n) = s.trim_end_matches(['p', 'P']).parse() {
        return n;
    }
    let up = link.to_ascii_uppercase();
    up.match_indices('P')
        .find_map(|(i, _)| {
            let start = up[..i].rfind(|c: char| !c.is_ascii_digit()).map_or(0, |p| p + 1);
            let d = &up[start..i];
            (3..=4).contains(&d.len()).then(|| d.parse().ok()).flatten()
        })
        .unwrap_or(1080)
}

/// Direkte Videodatei aus einem Ressourcen-Eintrag.
struct File {
    height: u32,
    url: String,
    resource_id: Option<String>,
}

/// Passt der Eintrag zur Folge? Beim Film (0/0) passt alles.
fn matches(item: &Value, season: usize, episode: usize) -> bool {
    (season, episode) == (0, 0) || (num(&item["se"]).unwrap_or(0), num(&item["ep"]).unwrap_or(0)) == (season, episode)
}

fn file(item: &Value) -> Option<File> {
    let url = item["resourceLink"].as_str().or(item["url"].as_str()).filter(|u| u.starts_with("http") && !is_placeholder(u))?;
    Some(File { height: height(&item["resolution"], url), url: url.to_string(), resource_id: text(&item["resourceId"]).or_else(|| text(&item["id"])) })
}

/// Direkte Dateien aus `resourceDetectors` der Titel-Daten.
fn detector_files(v: &Value, season: usize, episode: usize) -> Vec<File> {
    let mut out = Vec::new();
    for d in v["resourceDetectors"].as_array().into_iter().flatten() {
        match d["resolutionList"].as_array().filter(|l| !l.is_empty()) {
            Some(list) => out.extend(list.iter().filter(|i| matches(i, season, episode)).filter_map(file)),
            None if (season, episode) == (0, 0) => {
                let Some(url) = d["downloadUrl"].as_str().filter(|u| u.starts_with("http") && !is_placeholder(u)) else { continue };
                let link = d["resourceLink"].as_str().unwrap_or_default();
                out.push(File { height: height(&Value::Null, link), url: url.into(), resource_id: text(&d["resourceId"]) });
            }
            None => {}
        }
    }
    out
}

/// DASH-Stream aus play-info: Manifest-Adresse (meist aus dem CloudFront-Cookie), dazu Referer + Cookie.
struct Dash {
    height: u32,
    url: String,
    cookie: String,
    resource_id: Option<String>,
}

fn b64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let pad = "=".repeat((4 - s.len() % 4) % 4);
    base64::engine::general_purpose::STANDARD.decode(format!("{s}{pad}")).ok()
}

fn dash_mpd(url: &str) -> Option<String> {
    let base = url.trim_end_matches('*').trim_end_matches('/');
    (base.starts_with("http") && base.contains("/dash/")).then(|| format!("{base}/index.mpd"))
}

/// Manifest-Adresse aus `urlprefix=` oder der CloudFront-Policy (`Resource`) im Cookie.
fn manifest_from_cookie(cookie: &str) -> Option<String> {
    for part in cookie.split(';').map(str::trim) {
        if let Some((_, rest)) = part.split_once("urlprefix=") {
            let token = rest.split(':').next().unwrap_or(rest).trim().replace('-', "+").replace('_', "/");
            if let Some(m) = b64(&token).and_then(|b| String::from_utf8(b).ok()).and_then(|u| dash_mpd(&u)) {
                return Some(m);
            }
        }
        if let Some(policy) = part.strip_prefix("CloudFront-Policy=") {
            let p = policy.trim().replace('-', "+").replace('_', "=").replace('~', "/");
            let json = b64(&p).and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            if let Some(m) = json.as_ref().and_then(|j| j["Statement"][0]["Resource"].as_str()).and_then(dash_mpd) {
                return Some(m);
            }
        }
    }
    None
}

/// DASH-Streams und Zahl der Hinweis-Clips aus play-info.
fn dash_streams(v: &Value) -> (Vec<Dash>, usize) {
    let mut out = Vec::new();
    let mut placeholders = 0;
    for s in v["streams"].as_array().into_iter().flatten() {
        let url = s["url"].as_str().unwrap_or_default();
        if is_placeholder(url) {
            placeholders += 1;
        }
        let cookie = s["signCookie"].as_str().unwrap_or_default();
        let fallback = (url.starts_with("http") && !is_placeholder(url)).then(|| url.to_string());
        let Some(url) = manifest_from_cookie(cookie).or(fallback) else { continue };
        let res = s["resolutions"].as_str().or(v["displayResolutions"].as_str()).unwrap_or("1080");
        let height = res.split(',').filter_map(|r| r.trim().parse().ok()).max().unwrap_or(1080);
        let cookie = cookie.split(';').map(str::trim).filter(|c| !c.is_empty()).collect::<Vec<_>>().join("; ");
        out.push(Dash { height, url, cookie, resource_id: text(&s["id"]) });
    }
    (out, placeholders)
}

/// Untertitel (Name, Adresse), Englisch zuerst; ohne Mini-Dateien, Werbe-Untertitel und doppelte Sprachen.
fn pick_subtitles(v: &Value) -> Vec<(String, String)> {
    let mut caps: Vec<(String, String)> = v["extCaptions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| num(&c["size"]).is_none_or(|s| s == 0 || s > 100))
        .filter_map(|c| {
            let url = c["url"].as_str().filter(|u| !u.is_empty() && !u.contains("aa348f2541d13ffe"))?;
            let name = c["lanName"].as_str().filter(|n| !n.trim().is_empty()).or(c["lan"].as_str()).unwrap_or_default();
            Some((name.trim().to_string(), url.to_string()))
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    caps.retain(|(n, _)| seen.insert(n.to_lowercase()));
    caps.sort_by_key(|(n, _)| !n.to_lowercase().starts_with("en"));
    caps
}

/// Untertitel herunterladen (gleichzeitig) nach `<tmp>/boxqu-subs/<nr>-<hash>/<Name>.srt`; mpv zeigt den
/// Dateinamen als Spur-Titel. Im Temp-Verzeichnis, damit nichts liegen bleibt.
async fn download_subtitles(caps: Vec<(String, String)>) -> Vec<String> {
    let tasks: Vec<_> = caps
        .into_iter()
        .enumerate()
        .map(|(i, (name, url))| {
            tokio::spawn(async move {
                let bytes = HTTP.get(&url).send().await.ok()?.error_for_status().ok()?.bytes().await.ok()?;
                let dir = std::env::temp_dir().join("boxqu-subs").join(format!("{i}-{}", hash(&url)));
                let ext = url.split('?').next()?.rsplit_once('.').map_or("srt", |(_, e)| e);
                let name = name.replace(['/', ':', '\\'], " ");
                tokio::fs::create_dir_all(&dir).await.ok()?;
                let path = dir.join(format!("{name}.{ext}"));
                tokio::fs::write(&path, bytes).await.ok()?;
                Some(path.to_string_lossy().into_owned())
            })
        })
        .collect();
    let mut out = Vec::new();
    for t in tasks {
        out.extend(t.await.ok().flatten());
    }
    out
}

pub async fn stream(episode_id: String) -> Result<Stream, String> {
    let (id, season, episode) = parse_episode(&episode_id).ok_or("Invalid episode")?;
    let play_info = if (season, episode) == (0, 0) { format!("{API}/play-info/v2?subjectId={id}") } else { format!("{API}/play-info/v2?subjectId={id}&se={season}&ep={episode}") };
    let resources = format!(
        "{API}/resource/v2?subjectId={id}&page=1&perPage=20&all=0&startPosition=1&endPosition=1&pagerMode=2&resolution=0&se={season}&epFrom={episode}&epTo={episode}&isVip=true"
    );
    let subject_path = format!("{API}/get?subjectId={id}");
    let (subject, play, res) = tokio::join!(moviebox::get(&subject_path), moviebox::get(&play_info), moviebox::get(&resources));

    let mut files = subject.as_ref().map(|v| detector_files(v, season, episode)).unwrap_or_default();
    let (dash, mut placeholders) = play.as_ref().map(dash_streams).unwrap_or_default();
    // Ressourcen-IDs der Folge (auch von Hinweis-Clips): an ihnen hängen die Untertitel.
    let mut rids: Vec<String> = Vec::new();
    for item in res.as_ref().ok().and_then(|v| v["list"].as_array()).into_iter().flatten().filter(|i| matches(i, season, episode)) {
        rids.extend(text(&item["resourceId"]).or_else(|| text(&item["id"])));
        if let Some(f) = file(item) {
            files.push(f);
            continue;
        }
        // Nur Hinweis-Clip: echte Datei über `resource-position` nachfragen (wie die TUI).
        let rid = text(&item["resourceId"]).or_else(|| text(&item["id"]));
        let origin = item["resourceLink"].as_str().or(item["url"].as_str()).filter(|u| is_placeholder(u));
        let (Some(rid), Some(origin)) = (rid, origin) else { continue };
        placeholders += 1;
        let params = [("subjectId", id), ("resourceId", &rid), ("failUrl", origin), ("failCode", "404"), ("resourceNum", "0"), ("isVip", "true")];
        let q = reqwest::Url::parse_with_params("http://x/", &params).map_err(|e| e.to_string())?;
        if let Ok(v) = moviebox::get(&format!("{API}/resource-position/v2?{}", q.query().unwrap_or_default())).await {
            let found = v["list"].as_array().into_iter().flatten().find(|i| text(&i["resourceId"]).or_else(|| text(&i["id"])).as_deref() == Some(&rid));
            files.extend(found.and_then(file));
        }
    }

    // Direkte Dateien bevorzugen (eine Datei je Stufe, nur User-Agent nötig); DASH mit Cookie nur, wenn es
    // keine gibt oder es höher auflöst (Serien haben direkt oft nur 360p).
    let ua = ("User-Agent".to_string(), moviebox::user_agent().to_string());
    let best_file = files.iter().map(|f| f.height).max().unwrap_or(0);
    let dash: Option<Dash> = dash.into_iter().filter(|d| d.height > best_file).max_by_key(|d| d.height);
    // Untertitel-Kandidaten: Ressourcen-IDs der Dateien, dann der Liste, zuletzt die Stream-ID von play-info.
    let mut candidates: Vec<String> = files.iter().filter_map(|f| f.resource_id.clone()).collect();
    candidates.extend(rids);
    let (variants, headers) = if dash.is_none() && !files.is_empty() {
        files.sort_by(|a, b| b.height.cmp(&a.height));
        let mut variants: Vec<(u32, String)> = Vec::new();
        for f in files {
            if !variants.iter().any(|(h, _)| *h == f.height) {
                variants.push((f.height, f.url));
            }
        }
        (variants, vec![ua])
    } else if let Some(d) = dash {
        let mut headers = vec![("Referer".to_string(), moviebox::STREAM_REFERER.to_string()), ua];
        if !d.cookie.is_empty() {
            headers.push(("Cookie".to_string(), d.cookie));
        }
        candidates.extend(d.resource_id);
        // Über den lokalen Zwischenserver (lädt parallel vor, je Höhe ein Manifest); geht das nicht, direkt wie bisher.
        match crate::dash::serve(&d.url, headers.clone()).await {
            Some(variants) => (variants, vec![]),
            None => (vec![(d.height, d.url)], headers),
        }
    } else if placeholders > 0 {
        return Err("MovieBox is serving a placeholder clip instead of this title".into());
    } else {
        return Err(play.err().or(res.err()).unwrap_or_else(|| "No playable stream found".into()));
    };

    // Untertitel: nicht länger als 8 s darauf warten (das Video wartet so lange mit).
    let subtitles = tokio::time::timeout(Duration::from_secs(8), async {
        let caps = subtitle(id, season, episode, candidates).await.unwrap_or_default();
        download_subtitles(caps).await
    })
    .await
    .unwrap_or_default();
    Ok(Stream { variants, headers, subtitles, mal_id: None })
}

async fn captions(id: &str, rid: &str) -> Option<Vec<(String, String)>> {
    Some(pick_subtitles(&moviebox::get(&format!("{API}/get-ext-captions?subjectId={id}&resourceId={rid}")).await.ok()?)).filter(|c| !c.is_empty())
}

/// Erste Kandidaten-ID mit Untertiteln; sonst wie die TUI über die (alte) Ressourcen-Liste der Folge.
async fn subtitle(id: &str, season: usize, episode: usize, mut candidates: Vec<String>) -> Option<Vec<(String, String)>> {
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.clone()));
    for rid in candidates.iter().take(3) {
        if let Some(s) = captions(id, rid).await {
            return Some(s);
        }
    }
    // Die alte Liste filtert nicht nach Folge, sondern blättert die ganze Serie (20 pro Seite).
    let path = if (season, episode) == (0, 0) {
        format!("{API}/resource?subjectId={id}&page=1&perPage=20")
    } else {
        format!("{API}/resource?subjectId={id}&se={season}&ep={episode}&page={}&perPage=20", episode.saturating_sub(1) / 20 + 1)
    };
    let v = moviebox::get(&path).await.ok()?;
    let rid = v["list"].as_array()?.iter().filter(|i| matches(i, season, episode)).find_map(|i| text(&i["resourceId"]).or_else(|| text(&i["id"])))?;
    if candidates.contains(&rid) { None } else { captions(id, &rid).await }
}

// ---------- Poster ----------

fn cache_dir() -> PathBuf {
    qu_core::app::cache_dir()
}

fn hash(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:x}", h.finish())
}

/// Poster als Bytes; einmal geladen, kommt es von der Festplatte.
pub async fn poster(url: String) -> Result<Vec<u8>, String> {
    let path = cache_dir().join("posters").join(hash(&url));
    if let Ok(b) = tokio::fs::read(&path).await {
        return Ok(b);
    }
    let _slot = POSTER_SLOTS.acquire().await.map_err(|e| e.to_string())?;
    let res = HTTP.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let bytes = res.bytes().await.map_err(|e| e.to_string())?.to_vec();
    // ponytail: Cache wächst unbegrenzt (~30 KB pro Poster); Aufräumen nach Alter, falls es stört.
    let _ = tokio::fs::create_dir_all(path.parent().unwrap()).await;
    let _ = tokio::fs::write(&path, &bytes).await;
    Ok(bytes)
}

// ---------- Zuletzt geladene Liste je Bereich (sofort beim Start zeigen) ----------

fn source_cache(url: &str) -> PathBuf {
    cache_dir().join("sources").join(format!("{}.tsv", hash(url)))
}

pub fn saved_titles(url: &str) -> Vec<Title> {
    read_list(&source_cache(url))
}

pub fn save_titles(url: &str, list: &[Title]) {
    write_list(&source_cache(url), list);
}

/// Titel-Liste als TSV (id, Titel, Poster) – für Bereichs-Cache und Watchlist.
pub fn read_list(path: &std::path::Path) -> Vec<Title> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (id, title, poster) = (f.next()?, f.next()?, f.next()?);
            Some(Title { id: id.into(), title: title.into(), poster: (!poster.is_empty()).then(|| poster.into()), ..Default::default() })
        })
        .collect()
}

pub fn write_list(path: &std::path::Path, list: &[Title]) {
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    let text: String = list
        .iter()
        .map(|a| format!("{}\t{}\t{}\n", clean(&a.id), clean(&a.title), clean(a.poster.as_deref().unwrap_or_default())))
        .collect();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(path, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episode_ids() {
        assert_eq!(parse_episode("123/2/7"), Some(("123", 2, 7)));
        assert_eq!(parse_episode("123/0/0"), Some(("123", 0, 0)));
        assert_eq!(parse_episode("123"), None);
    }

    #[test]
    fn parses_subjects_and_seasons() {
        let home = json!({"items": [
            {"banner": {"banners": [{"subject": {"subjectId": 1, "title": "A", "releaseDate": "2024-05-01", "subjectType": 2, "cover": {"url": "https://p/a.jpg"}}}]}},
            {"subjects": [{"subjectId": "2", "title": "B"}, {"subjectId": 1, "title": "A"}]}
        ]});
        let t = homepage_titles(&home);
        assert_eq!(t.len(), 2, "doppelte IDs nur einmal");
        assert_eq!((t[0].id.as_str(), t[0].kind.as_deref(), t[0].poster.as_deref()), ("1", Some("2024 · Series"), Some("https://p/a.jpg")));
        assert_eq!(search_titles(&json!({"results": [{"subjects": [{"subjectId": 9, "title": "X"}]}]}))[0].id, "9");
        let d = json!({"seasons": {"seasons": [{"se": 1, "maxEp": 2}, {"se": 2, "episodeNumbers": [1, 3]}]}});
        assert_eq!(parse_episodes(&d), vec![(1, 1), (1, 2), (2, 1), (2, 3)]);
        let det = parse_details(&json!({"subjectId": 5, "title": "F", "genre": "Action, Drama", "imdbRatingValue": "7.4", "duration": 5400,
            "dubs": [{"lanCode": "en", "lanName": "Original Audio", "original": true, "subjectId": "6"}, {"lanCode": "hi", "lanName": "Hindi dub", "subjectId": 5},
                     {"lanCode": "esla", "lanName": "esla dub", "subjectId": "7"}]})).unwrap();
        assert_eq!((det.genres.len(), det.info[0].as_str(), det.item.duration.as_deref()), (2, "IMDb 7.4", Some("90m")));
        let names: Vec<(&str, &str)> = det.languages.iter().map(|l| (l.name.as_str(), l.id.as_str())).collect();
        assert_eq!(names, vec![("Original", "6"), ("Hindi", "5"), ("Spanish (Latin America)", "7")]);
    }

    #[test]
    fn clean_titles() {
        assert_eq!(clean_title("Ranabaali [Hindi]"), "Ranabaali");
        assert_eq!(clean_title("Breaking Bad [Hindi] S5"), "Breaking Bad");
        assert_eq!(clean_title("Ramayana [Hindi] [CAM]"), "Ramayana");
        assert_eq!(clean_title("Dark S1-3"), "Dark");
        assert_eq!(clean_title("Superman Saga"), "Superman Saga", "nur „S“ + Ziffern ist eine Staffel");
        assert_eq!(clean_title("[Hindi]"), "[Hindi]", "nichts übrig: Original behalten");
        let mut out = Vec::new();
        for (id, title) in [("1", "X [Hindi]"), ("2", "X"), ("3", "Y")] {
            push_unique(&mut out, subject(&json!({"subjectId": id, "title": title})).unwrap());
        }
        assert_eq!(out.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["1", "3"], "Sprachfassungen nur einmal");
    }

    #[test]
    fn streams() {
        assert_eq!(height(&json!(720), ""), 720);
        assert_eq!(height(&json!("4K"), ""), 2160);
        assert_eq!(height(&Value::Null, "https://x/Film_480P.mp4"), 480);
        assert!(is_placeholder("https://macdn.aoneroom.com/other/2026/x.mp4"));
        // CloudFront-Policy mit Resource …/dash/…* → Manifest.
        use base64::Engine;
        let policy = base64::engine::general_purpose::STANDARD.encode(r#"{"Statement":[{"Resource":"https://cdn/dash/abc/*"}]}"#);
        let policy = policy.replace('+', "-").replace('=', "_").replace('/', "~");
        assert_eq!(manifest_from_cookie(&format!("CloudFront-Policy={policy}; CloudFront-Signature=s")).as_deref(), Some("https://cdn/dash/abc/index.mpd"));
        let caps = json!({"extCaptions": [{"url": "https://s/fr.srt", "lanName": "French"}, {"url": "https://s/en.srt", "lanName": "English"},
            {"url": "https://s/tiny.srt", "lanName": "English", "size": 20}, {"url": "https://s/en2.srt", "lanName": "english"}]});
        let names: Vec<String> = pick_subtitles(&caps).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["English", "French"], "Englisch zuerst, Mini-Datei und doppelte Sprache weg");
    }

    /// Live gegen MovieBox (nur von Hand: `cargo test -- --ignored`).
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_moviebox() {
        let list = titles(sources().remove(0), 3).await.unwrap();
        assert!(!list.is_empty());
        let id = list[0].id.clone();
        let d = details(id.clone()).await.unwrap();
        println!("{} | {:?} | {:?} | {:?}", d.item.title, d.item.kind, d.info, d.genres);
        let eps = episodes(id).await.unwrap();
        let s = stream(eps[0].id.clone()).await.unwrap();
        let names = |s: &Stream| s.headers.iter().map(|h| h.0.clone()).collect::<Vec<_>>();
        println!("{} eps, {:?}, headers {:?}, subs {:?}", eps.len(), s.variants.iter().map(|v| v.0).collect::<Vec<_>>(), names(&s), s.subtitles);
        assert!(!s.variants.is_empty());
        // Sprachfassungen: von der Hindi-Fassung zur Original-Fassung (so springt die Titel-Seite um).
        let found = search("breaking bad".into()).await.unwrap();
        let d = details(found[0].id.clone()).await.unwrap();
        let original = d.languages.iter().find(|l| l.original).expect("Original-Fassung");
        let o = details(original.id.clone()).await.unwrap();
        println!("{} → {} ({:?})", d.item.title, o.item.title, d.languages.iter().map(|l| l.name.as_str()).collect::<Vec<_>>());
        let s = stream(episodes(original.id.clone()).await.unwrap()[0].id.clone()).await.unwrap();
        println!("Original S1E1: {:?}, sub {}", s.variants.iter().map(|v| v.0).collect::<Vec<_>>(), s.subtitles.len());
        let series = found.iter().find(|t| t.kind.as_deref().is_some_and(|k| k.contains("Series"))).expect("Serie gefunden");
        let eps = episodes(series.id.clone()).await.unwrap();
        println!("{}: {} eps, erste {}", series.title, eps.len(), eps[0].title);
        let s = stream(eps[0].id.clone()).await.unwrap();
        println!("Serie: {:?}, headers {:?}, sub {}", s.variants.iter().map(|v| v.0).collect::<Vec<_>>(), names(&s), s.subtitles.len());
    }
}
