//! Eigener minimalistischer Icon-Satz (data/icons). Die SVGs stecken im Programm und werden beim
//! Start in den Cache geschrieben und GTK bekannt gemacht. Endung `-symbolic`: GTK färbt sie in
//! der aktuellen Schriftfarbe ein, sie passen sich also dem Theme an.

use gtk4 as gtk;
use std::path::PathBuf;

const ICONS: [(&str, &str); 17] = [
    ("boxqu-search-symbolic", include_str!("../data/icons/boxqu-search-symbolic.svg")),
    ("boxqu-home-symbolic", include_str!("../data/icons/boxqu-home-symbolic.svg")),
    ("boxqu-back-symbolic", include_str!("../data/icons/boxqu-back-symbolic.svg")),
    ("boxqu-next-symbolic", include_str!("../data/icons/boxqu-next-symbolic.svg")),
    ("boxqu-play-symbolic", include_str!("../data/icons/boxqu-play-symbolic.svg")),
    ("boxqu-pause-symbolic", include_str!("../data/icons/boxqu-pause-symbolic.svg")),
    ("boxqu-volume-symbolic", include_str!("../data/icons/boxqu-volume-symbolic.svg")),
    ("boxqu-mute-symbolic", include_str!("../data/icons/boxqu-mute-symbolic.svg")),
    ("boxqu-settings-symbolic", include_str!("../data/icons/boxqu-settings-symbolic.svg")),
    ("boxqu-fullscreen-symbolic", include_str!("../data/icons/boxqu-fullscreen-symbolic.svg")),
    ("boxqu-restore-symbolic", include_str!("../data/icons/boxqu-restore-symbolic.svg")),
    ("boxqu-reload-symbolic", include_str!("../data/icons/boxqu-reload-symbolic.svg")),
    ("boxqu-subtitles-symbolic", include_str!("../data/icons/boxqu-subtitles-symbolic.svg")),
    ("boxqu-spinner-symbolic", include_str!("../data/icons/boxqu-spinner-symbolic.svg")),
    ("boxqu-close-symbolic", include_str!("../data/icons/boxqu-close-symbolic.svg")),
    ("boxqu-watchlist-symbolic", include_str!("../data/icons/boxqu-watchlist-symbolic.svg")),
    ("boxqu-watchlisted-symbolic", include_str!("../data/icons/boxqu-watchlisted-symbolic.svg")),
];

/// Lade-Kreis: eigenes Icon, per CSS gedreht (`.boxqu-spinner` in style.css), sichtbar = läuft.
/// Statt `gtk::Spinner`: der friert in GTK 4.24 ein, sobald er einmal ausgeblendet war
/// (auch durch Seitenwechsel), und dreht sich nach dem Wiedereinblenden nicht mehr.
pub fn spinner(size: i32) -> gtk::builders::ImageBuilder {
    gtk::Image::builder().icon_name("boxqu-spinner-symbolic").pixel_size(size).css_classes(["boxqu-spinner"])
}

pub fn install(display: &gtk::gdk::Display) {
    // Lose Dateien direkt im Suchpfad findet GTK ohne Theme-Struktur.
    let dir = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/boxqu/icons");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (name, svg) in ICONS {
        let _ = std::fs::write(dir.join(format!("{name}.svg")), svg);
    }
    gtk::IconTheme::for_display(display).add_search_path(dir);
}
