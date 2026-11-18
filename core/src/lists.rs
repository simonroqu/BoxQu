//! Kleine lokale Listen in `~/.local/share/boxqu/`: Watchlist, Geschwindigkeit pro Titel.

use qu_core::app::data_file;
use qu_core::store::{read, write};

// ---------- Watchlist (neueste zuerst) ----------

pub fn watchlist() -> Vec<crate::source::Title> {
    crate::source::read_list(&data_file("watchlist.tsv"))
}

pub fn on_watchlist(id: &str) -> bool {
    watchlist().iter().any(|a| a.id == id)
}

/// Aufnehmen bzw. entfernen; liefert, ob er jetzt drauf ist.
pub fn toggle_watchlist(a: &crate::source::Title) -> bool {
    let mut list = watchlist();
    let before = list.len();
    list.retain(|x| x.id != a.id);
    let added = list.len() == before;
    if added {
        list.insert(0, a.clone());
    }
    crate::source::write_list(&data_file("watchlist.tsv"), &list);
    added
}

// ---------- Werte pro Titel (Tabelle „id<TAB>wert“) ----------

/// Zuletzt für diesen Titel gewählte Geschwindigkeit (sonst normal).
pub fn anime_speed(anime: &str) -> f64 {
    qu_core::store::speed(anime)
}

pub fn set_anime_speed(anime: &str, speed: f64) {
    qu_core::store::set_speed(anime, speed);
}

/// Bevorzugte Sprachfassung, für alle Titel: `original` (Standard) oder ein MovieBox-`lanCode`.
pub fn language() -> String {
    Some(read("language").trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "original".into())
}

pub fn set_language(code: &str) {
    write("language", code);
}

/// Die Fassung, die zur Vorliebe passt (sonst keine: dann bleibt die geöffnete).
pub fn preferred(languages: &[crate::source::Language]) -> Option<&crate::source::Language> {
    let want = language();
    languages.iter().find(|l| if want == "original" { l.original } else { l.code == want })
}

