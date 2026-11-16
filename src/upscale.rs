//! Upscaling beim Zeichnen (mpv auf der GPU): „Sharp“ = besserer Skalierfilter, „AI“ = FSRCNNX (GLSL-Shader,
//! eingebettet, Lizenz in `data/shaders/`). Wirkt nur, wenn das Bild größer gezeichnet wird als das Video
//! (z. B. 1080p im Vollbild auf 1440p). NVIDIAs RTX Video Super Resolution kann mpv unter Linux nicht ansprechen.

use libmpv2::Mpv;
use std::path::PathBuf;

/// Shader für „AI“, in dieser Reihenfolge (Name, Inhalt).
const SHADERS: [(&str, &str); 1] = [
    ("FSRCNNX_x2_16-0-4-1.glsl", include_str!("../data/shaders/FSRCNNX_x2_16-0-4-1.glsl")),
];

/// (Wert in der Datei, Text im Menü).
pub const MODES: [(&str, &str); 3] = [("off", "Off"), ("sharp", "Sharp"), ("ai", "AI (FSRCNNX)")];

fn shader_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/boxqu/shaders")
}

/// Gespeicherte Stufe (`~/.config/boxqu/upscaling`), Standard „off“.
pub fn saved() -> &'static str {
    let s = std::fs::read_to_string(crate::controls::config_file("upscaling")).unwrap_or_default();
    MODES.iter().map(|(m, _)| *m).find(|m| *m == s.trim()).unwrap_or("off")
}

/// Stufe anwenden (geht auch während das Video läuft).
pub fn apply(mpv: &Mpv, mode: &str) {
    let _ = mpv.set_property("scale", if mode == "off" { "lanczos" } else { "ewa_lanczossharp" });
    let _ = mpv.command("change-list", &["glsl-shaders", "clr", ""]);
    if mode != "ai" {
        return;
    }
    // mpv liest Shader nur aus Dateien: einmal in den Cache schreiben (bzw. nach einem Update neu).
    let dir = shader_dir();
    let _ = std::fs::create_dir_all(&dir);
    for (name, text) in SHADERS {
        let path = dir.join(name);
        if std::fs::metadata(&path).map(|m| m.len()).ok() != Some(text.len() as u64) {
            let _ = std::fs::write(&path, text);
        }
        let _ = mpv.command("change-list", &["glsl-shaders", "append", &path.to_string_lossy()]);
    }
}

/// Stufe wählen: anwenden und merken.
pub fn set(mpv: &Mpv, mode: &str) {
    apply(mpv, mode);
    let f = crate::controls::config_file("upscaling");
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let _ = std::fs::write(f, mode);
}
