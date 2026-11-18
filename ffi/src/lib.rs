//! boxqu-core für Kotlin (UniFFI). Nur Durchreichen: eigene Typen als `Record`, Netz-Aufrufe laufen auf
//! qu-cores tokio (`bg`), Kotlin wartet per `suspend`. Zuerst `init` aufrufen (Ordner, qu.toml).

use boxqu_core::{lists, progress, source};

uniffi::setup_scaffolding!();

#[derive(Debug, uniffi::Error)]
pub enum BoxError {
    Failed { msg: String },
}

impl std::fmt::Display for BoxError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let BoxError::Failed { msg } = self;
        f.write_str(msg)
    }
}

impl From<String> for BoxError {
    fn from(msg: String) -> Self {
        BoxError::Failed { msg }
    }
}

#[derive(uniffi::Record)]
pub struct Title {
    pub id: String,
    pub title: String,
    pub poster: Option<String>,
    /// Jahr o. Ä.
    pub kind: Option<String>,
    pub duration: Option<String>,
}

impl From<source::Title> for Title {
    fn from(t: source::Title) -> Self {
        Title { id: t.id, title: t.title, poster: t.poster, kind: t.kind, duration: t.duration }
    }
}

impl From<Title> for source::Title {
    fn from(t: Title) -> Self {
        source::Title { id: t.id, title: t.title, poster: t.poster, kind: t.kind, duration: t.duration }
    }
}

#[derive(Clone, uniffi::Record)]
pub struct Language {
    pub code: String,
    pub name: String,
    /// Titel-ID dieser Fassung.
    pub id: String,
    pub original: bool,
}

impl From<&Language> for source::Language {
    fn from(l: &Language) -> Self {
        source::Language { code: l.code.clone(), name: l.name.clone(), id: l.id.clone(), original: l.original }
    }
}

#[derive(uniffi::Record)]
pub struct Details {
    pub item: Title,
    pub description: String,
    pub info: Vec<String>,
    pub genres: Vec<String>,
    /// Sprachfassungen; weniger als zwei = kein Umschalter.
    pub languages: Vec<Language>,
}

#[derive(uniffi::Record)]
pub struct Episode {
    pub id: String,
    pub number: u32,
    /// „S1 E3“ bzw. Episodenname.
    pub title: String,
}

#[derive(uniffi::Record)]
pub struct Variant {
    pub height: u32,
    pub url: String,
}

#[derive(uniffi::Record)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(uniffi::Record)]
pub struct Stream {
    /// Beste zuerst; DASH über den lokalen Zwischenserver (`http://127.0.0.1:…/index.mpd`) oder direkte Dateien.
    pub variants: Vec<Variant>,
    pub headers: Vec<Header>,
    /// Lokale SRT-Dateien, Dateiname = Sprache, bevorzugte zuerst.
    pub subtitles: Vec<String>,
}

#[derive(uniffi::Record)]
pub struct Section {
    pub url: String,
    pub name: String,
}

#[derive(uniffi::Record)]
pub struct Resume {
    pub item: Title,
    pub episode: u32,
    pub fraction: f64,
    /// 1 = Film (Karte ohne Episodennummer).
    pub total: u32,
}

#[derive(uniffi::Record)]
pub struct Watched {
    pub fraction: f64,
    pub finished: bool,
}

fn list(v: Vec<source::Title>) -> Vec<Title> {
    v.into_iter().map(Title::from).collect()
}

/// Einmal beim Start: Ordner der App (`filesDir`, `cacheDir`) und Einstellungen aus qu.toml.
#[uniffi::export]
pub fn init(files_dir: String, cache_dir: String) {
    // Untertitel landen in temp_dir(); Androids Standard (/data/local/tmp) darf die App nicht beschreiben.
    // SAFETY: einmal beim Start, bevor Threads (tokio) laufen.
    unsafe { std::env::set_var("TMPDIR", &cache_dir) };
    qu_core::app::set_dirs(files_dir.into(), cache_dir.into());
    qu_core::app::init("boxqu");
    qu_core::config::load(include_str!("../../qu.toml"));
}

/// Feste Bereiche der Startseite (derzeit „Trending“).
#[uniffi::export]
pub fn sections() -> Vec<Section> {
    source::sources().into_iter().map(|s| Section { url: s.url, name: s.name }).collect()
}

/// Titel eines Bereichs (aus dem Netz, danach gemerkt für `saved_titles`).
#[uniffi::export]
pub async fn titles(section: Section, limit: u32) -> Result<Vec<Title>, BoxError> {
    let url = section.url.clone();
    let src = source::Source { url: section.url, name: section.name };
    let v = qu_core::bg(source::titles(src, limit as usize)).await?;
    source::save_titles(&url, &v);
    Ok(list(v))
}

/// Zuletzt geladene Titel eines Bereichs (sofort, ohne Netz).
#[uniffi::export]
pub fn saved_titles(url: String) -> Vec<Title> {
    list(source::saved_titles(&url))
}

#[uniffi::export]
pub async fn search(query: String) -> Result<Vec<Title>, BoxError> {
    Ok(list(qu_core::bg(source::search(query)).await?))
}

#[uniffi::export]
pub async fn details(id: String) -> Result<Details, BoxError> {
    let d = qu_core::bg(source::details(id)).await?;
    Ok(Details {
        item: d.item.into(),
        description: d.description,
        info: d.info,
        genres: d.genres,
        languages: d
            .languages
            .into_iter()
            .map(|l| Language { code: l.code, name: l.name, id: l.id, original: l.original })
            .collect(),
    })
}

#[uniffi::export]
pub async fn episodes(id: String) -> Result<Vec<Episode>, BoxError> {
    let v = qu_core::bg(source::episodes(id)).await?;
    Ok(v.into_iter().map(|e| Episode { id: e.id, number: e.number, title: e.title }).collect())
}

#[uniffi::export]
pub async fn stream(episode_id: String) -> Result<Stream, BoxError> {
    let s = qu_core::bg(source::stream(episode_id)).await?;
    Ok(Stream {
        variants: s.variants.into_iter().map(|(height, url)| Variant { height, url }).collect(),
        headers: s.headers.into_iter().map(|(name, value)| Header { name, value }).collect(),
        subtitles: s.subtitles,
    })
}

/// Bevorzugte Sprachfassung (zuletzt gewählte Sprache) unter `languages`, falls vorhanden.
#[uniffi::export]
pub fn preferred(languages: Vec<Language>) -> Option<Language> {
    let core: Vec<source::Language> = languages.iter().map(source::Language::from).collect();
    let id = lists::preferred(&core)?.id.clone();
    languages.into_iter().find(|l| l.id == id)
}

/// Sprache merken (Tab-Klick): `original` oder der Sprachcode.
#[uniffi::export]
pub fn set_language(l: Language) {
    lists::set_language(if l.original { "original" } else { &l.code });
}

#[uniffi::export]
pub fn continue_watching(n: u32) -> Vec<Resume> {
    progress::continue_watching(n as usize)
        .into_iter()
        .map(|r| Resume { item: r.anime.into(), episode: r.episode, fraction: r.fraction, total: r.total })
        .collect()
}

#[uniffi::export]
pub fn resume_at(id: String, episode: u32) -> Option<f64> {
    progress::resume_at(&id, episode)
}

/// Fortschritt speichern. `total`: Episodenzahl (1 = Film).
#[uniffi::export]
pub fn record(t: Title, episode: u32, total: u32, pos: f64, dur: f64) {
    let m = progress::Meta { anime: t.id, episode, title: t.title, poster: t.poster, total };
    progress::record(&m, pos, dur);
}

#[uniffi::export]
pub fn watched(id: String, episode: u32) -> Option<Watched> {
    progress::get(&id, episode).map(|e| Watched { fraction: e.fraction(), finished: e.finished() })
}

/// „Watched“: aus „Continue watching“ nehmen.
#[uniffi::export]
pub fn hide(id: String) {
    progress::hide(&id);
}

#[uniffi::export]
pub fn watchlist() -> Vec<Title> {
    list(lists::watchlist())
}

#[uniffi::export]
pub fn on_watchlist(id: String) -> bool {
    lists::on_watchlist(&id)
}

#[uniffi::export]
pub fn toggle_watchlist(t: Title) -> bool {
    lists::toggle_watchlist(&t.into())
}

#[uniffi::export]
pub fn speed(id: String) -> f64 {
    lists::anime_speed(&id)
}

#[uniffi::export]
pub fn searches() -> Vec<String> {
    qu_core::store::searches()
}

#[uniffi::export]
pub fn add_search(query: String) {
    qu_core::store::add_search(&query);
}
