//! Eigene Bedienleiste über dem Video: Fortschritt (mit Kapiteln), Play/Pause, Lautstärke,
//! Einstellungen (Qualität, Geschwindigkeit), Vollbild.
//! Bubble-Design: Timeline ohne Hintergrund, jede Knopfgruppe in einer eigenen Glas-Pille.
//! Erscheint nur bei Mausbewegung über dem Player und blendet sich nach kurzer Zeit wieder aus.

use crate::grid::fmt_duration;
use gtk4 as gtk;
use gtk::glib;
use gtk::prelude::*;
use libmpv2::Mpv;
use crate::aniskip::Segment;
use libmpv2::events::PropertyData;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

const HIDE_AFTER: Duration = Duration::from_millis(2000);
/// Beim Ziehen an der Timeline: so lange still halten, bevor gesprungen wird.
const SEEK_DELAY: Duration = Duration::from_millis(150);

/// Wählbare Qualitäten (maximale Bildhöhe). Gibt es die gewählte nicht, nimmt der Player die nächstkleinere.
// 4K gibt MovieBox bei manchen Filmen (z. B. Interstellar), 1440p bisher nie (Stand 2026-11) – steht für den Fall drin.
const QUALITIES: [(u32, &str); 6] = [(2160, "4K"), (1440, "1440p"), (1080, "1080p"), (720, "720p"), (480, "480p"), (360, "360p")];
const DEFAULT_QUALITY: u32 = 1080;
pub const SPEEDS: [f64; 8] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// Datei in `~/.config/boxqu/` (Qualität, Lautstärke).
pub fn config_file(name: &str) -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
    base.join("boxqu").join(name)
}

fn quality_file() -> PathBuf {
    config_file("quality")
}

/// Ein-/Ausblenden des Tons und Lautstärke-Rampen: so viele Schritte in diesem Abstand (zusammen 160 ms).
const FADE_STEPS: u32 = 8;
const FADE_STEP: Duration = Duration::from_millis(20);

/// Höchste Lautstärke in % (Regler-Ende). 100 = normal, liegt so in der Mitte des Reglers.
pub const MAX_VOLUME: f64 = 200.0;

/// Zuletzt eingestellte Lautstärke (0–`MAX_VOLUME`), bleibt über Neustarts erhalten.
/// Standard 100 = normale Lautstärke, der Regler steht dann in der Mitte.
pub fn saved_volume() -> f64 {
    std::fs::read_to_string(config_file("volume"))
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map_or(100.0, |v| v.clamp(0.0, MAX_VOLUME))
}

fn save_volume(v: f64) {
    let f = config_file("volume");
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let _ = std::fs::write(f, format!("{v:.0}"));
}

/// Zuletzt gewählte Qualität (bleibt über Neustarts erhalten).
pub fn saved_quality() -> u32 {
    std::fs::read_to_string(quality_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .filter(|q| QUALITIES.iter().any(|(h, _)| h == q))
        .unwrap_or(DEFAULT_QUALITY)
}

pub fn speed_label(s: f64) -> String {
    if s == 1.0 { "Normal".into() } else { format!("{s}×") }
}

/// Kapitel, in dem `pos` liegt (Kapitel sind nach Startzeit sortiert).
fn chapter_at(chapters: &[(f64, String)], pos: f64) -> Option<&str> {
    chapters.iter().rev().find(|(start, _)| *start <= pos).map(|(_, t)| t.as_str())
}

pub struct Controls {
    pub widget: gtk::Revealer,
    /// Vorschau über der Timeline; liegt als eigene Ebene im Player (player.rs), nicht in der Leiste.
    pub preview: gtk::Box,
    pub fullscreen: gtk::Button,
    mpv: &'static Mpv,
    pub play: gtk::Button,
    seek: gtk::Scale,
    time: gtk::Label,
    vol: gtk::Scale,
    pub mute: gtk::Button,
    settings: gtk::MenuButton,
    /// Linksklick/Taste c: an/aus; Rechtsklick: Menü mit Sprache und Aussehen (`sub_menu`).
    pub subtitles: gtk::Button,
    sub_menu: gtk::Popover,
    sub_prefs: Rc<RefCell<SubPrefs>>,
    speed_buttons: Vec<(f64, gtk::CheckButton)>,
    /// Bekommt beim Ausblenden einen unsichtbaren Mauszeiger.
    cursor_target: gtk::Widget,
    hide_timer: RefCell<Option<glib::SourceId>>,
    chapters: Rc<RefCell<Vec<(f64, String)>>>,
    /// Opening/Ending (ani-skip), als dezente Bereiche über dem Fortschrittsbalken gezeichnet.
    segments: Rc<RefCell<Vec<Segment>>>,
    /// Bis hierhin (Sekunden) ist das Video vorgeladen; wird hell in der Timeline gezeigt.
    cached_until: Rc<Cell<f64>>,
    /// Wird mit der neuen Höhe aufgerufen, wenn die Qualität geändert wird (Player lädt neu).
    on_quality: Rc<RefCell<Option<Box<dyn Fn(u32)>>>>,
    segment_layer: gtk::DrawingArea,
    pos: Cell<f64>,
    /// Ziel eines Timeline-Sprungs und wann er an mpv ging (`None` = wartet noch auf `SEEK_DELAY`).
    seek_target: Rc<Cell<Option<(f64, Option<Instant>)>>>,
    dur: Cell<f64>,
    speed: Cell<f64>,
    /// Ton aus (von mpv gemeldet) und laufende Ein-/Ausblendung bzw. Lautstärke-Rampe (Nummer, damit eine
    /// neuere die ältere abbricht).
    muted: Cell<bool>,
    fade_gen: Cell<u32>,
    vol_gen: Cell<u32>,
    vol_target: Cell<Option<f64>>,
    live: Cell<bool>,
    paused: Cell<bool>,
}

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tip)
        .css_classes(["flat"])
        // Kein Fokus, sonst würde die Leertaste den zuletzt geklickten Button auslösen.
        .focusable(false)
        .build()
}

/// Abstand der Vorschau über der Timeline-Schiene.
const PREVIEW_GAP: f32 = 24.0;

/// Vorschau mittig über der Maus (nicht über die Enden der Timeline hinaus) und immer gleich hoch über
/// der Schiene. Gerechnet in der Ebene, in der sie liegt: so verschieben sie weder Kapitel-Striche
/// unter der Schiene noch eine höhere Vorschau.
fn place_preview(preview: &gtk::Box, seek: &gtk::Scale, trough: &gtk::Widget, x: f64) {
    let Some(layer) = preview.parent() else { return };
    let (Some(tr), Some(sk)) = (trough.compute_bounds(&layer), seek.compute_bounds(&layer)) else { return };
    let w = preview.width().max(preview.first_child().map_or(0, |c| c.width_request())).max(60) as f64;
    let lo = sk.x() as f64;
    let hi = (lo + sk.width() as f64 - w).max(lo);
    preview.set_margin_start((lo + x - w / 2.0).clamp(lo, hi) as i32);
    preview.set_margin_bottom((layer.height() as f32 - tr.y() + PREVIEW_GAP).max(0.0) as i32);
}

fn slider(max: f64) -> gtk::Scale {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, max, 1.0);
    s.set_draw_value(false);
    s.set_focusable(false);
    s
}

/// Spalte im Einstellungsmenü: Überschrift und Auswahlknöpfe (nur einer aktiv).
/// Untertitel-Schriftgrößen (mpv `sub-font-size`, Standard 38).
const SUB_SIZES: [(i64, &str); 4] = [(30, "Small"), (38, "Normal"), (48, "Large"), (60, "Huge")];

/// Untertitel-Einstellungen in `~/.config/boxqu/subtitles`, je eine Zeile: Sprache (Spur-Name, z. B. „English“),
/// Größe, Hintergrund (`box`/`outline`), an/aus (`on`/`off`). Wie in Tubequ, plus an/aus über Videos hinweg.
#[derive(Debug, PartialEq)]
struct SubPrefs {
    lang: String,
    size: i64,
    boxed: bool,
    on: bool,
}

impl SubPrefs {
    fn parse(s: &str) -> Self {
        let mut l = s.lines().map(str::trim);
        let lang = l.next().filter(|x| !x.is_empty()).unwrap_or("English").to_string();
        let size = l.next().and_then(|x| x.parse().ok()).filter(|v| SUB_SIZES.iter().any(|(s, _)| s == v)).unwrap_or(38);
        let boxed = l.next() == Some("box");
        SubPrefs { lang, size, boxed, on: l.next() != Some("off") }
    }

    fn load() -> Self {
        Self::parse(&std::fs::read_to_string(config_file("subtitles")).unwrap_or_default())
    }

    fn save(&self) {
        let f = config_file("subtitles");
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let style = if self.boxed { "box" } else { "outline" };
        let _ = std::fs::write(f, format!("{}\n{}\n{style}\n{}\n", self.lang, self.size, if self.on { "on" } else { "off" }));
    }

    fn apply(&self, mpv: &Mpv) {
        let _ = mpv.set_property("sub-font-size", self.size);
        let _ = mpv.set_property("sub-border-style", if self.boxed { "background-box" } else { "outline-and-shadow" });
    }
}

/// Untertitelspuren des Videos: (mpv-ID, Name). Name = Dateiname ohne Endung („English.srt“ → „English“),
/// sonst die Sprache. Nur bei Klicks/Laden abgefragt, nicht in Ereignissen.
fn sub_tracks(mpv: &Mpv) -> Vec<(i64, String)> {
    let n = mpv.get_property::<i64>("track-list/count").unwrap_or(0);
    let prop = |i: i64, k: &str| mpv.get_property::<String>(&format!("track-list/{i}/{k}")).unwrap_or_default();
    (0..n)
        .filter(|i| prop(*i, "type") == "sub")
        .filter_map(|i| {
            let id = mpv.get_property::<i64>(&format!("track-list/{i}/id")).ok()?;
            let title = prop(i, "title");
            let name = title.rsplit_once('.').map_or(title.as_str(), |(n, _)| n).to_string();
            let name = [name, prop(i, "lang")].into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| "Subtitles".into());
            Some((id, name))
        })
        .collect()
}

/// Spur für die gewünschte Sprache: genau diese, sonst Englisch, sonst die erste.
fn pick_sub(tracks: &[(i64, String)], lang: &str) -> Option<i64> {
    tracks
        .iter()
        .find(|(_, l)| l.eq_ignore_ascii_case(lang))
        .or_else(|| tracks.iter().find(|(_, l)| l.to_lowercase().starts_with("en")))
        .or(tracks.first())
        .map(|(id, _)| *id)
}

fn radio_column(title: &str, labels: &[String]) -> (gtk::Box, Vec<gtk::CheckButton>) {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 2);
    col.append(&gtk::Label::builder().label(title).xalign(0.0).margin_bottom(4).css_classes(["heading"]).build());
    let mut buttons: Vec<gtk::CheckButton> = Vec::new();
    for l in labels {
        let b = gtk::CheckButton::builder().label(l).focusable(false).build();
        if let Some(first) = buttons.first() {
            b.set_group(Some(first));
        }
        col.append(&b);
        buttons.push(b);
    }
    (col, buttons)
}

impl Controls {
    pub fn new(mpv: &'static Mpv, cursor_target: &impl IsA<gtk::Widget>) -> Rc<Self> {
        let play = icon_button("boxqu-pause-symbolic", "Pause (space)");
        play.connect_clicked(move |_| {
            let _ = mpv.command("cycle", &["pause"]);
        });

        let seek = slider(1.0);
        seek.set_hexpand(true);

        let mute = icon_button("boxqu-volume-symbolic", "Mute (m)");
        let vol = slider(MAX_VOLUME);
        vol.set_width_request(90);
        vol.set_margin_end(8);
        vol.connect_change_value(move |_, _, v| {
            let _ = mpv.set_property("volume", v.clamp(0.0, MAX_VOLUME));
            glib::Propagation::Proceed
        });

        let time = gtk::Label::builder()
            .css_classes(["numeric"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let fullscreen = icon_button("boxqu-fullscreen-symbolic", "Full screen (f)");

        // Timeline: change-value kommt nur von der Maus, nicht von set_value (keine Rückkopplung).
        // Beim Ziehen nicht bei jeder Bewegung springen (jeder Sprung lädt neu), sondern erst,
        // wenn die Maus kurz still steht oder losgelassen wird. Die Zeit folgt sofort.
        let pending_seek: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
        // Sprungziel, bis mpv dort ankommt: so lange zeigt die Timeline nicht die alte Position (`seek_state`).
        let seek_target: Rc<Cell<Option<(f64, Option<Instant>)>>> = Rc::default();
        seek.connect_change_value({
            let time = time.downgrade();
            let seek_target = seek_target.clone();
            move |s, _, v| {
                seek_target.set(Some((v, None)));
                if let Some(t) = time.upgrade() {
                    t.set_text(&format!("{} / {}", fmt_duration(v.max(0.0) as u32), fmt_duration(s.adjustment().upper() as u32)));
                }
                if let Some(id) = pending_seek.take() {
                    id.remove();
                }
                let (pending, seek_target) = (pending_seek.clone(), seek_target.clone());
                let id = glib::timeout_add_local_once(SEEK_DELAY, move || {
                    pending.take();
                    seek_target.set(Some((v, Some(Instant::now()))));
                    let _ = mpv.command("seek", &[&v.to_string(), "absolute"]);
                });
                pending_seek.replace(Some(id));
                glib::Propagation::Proceed
            }
        });

        // Einstellungen: Qualität und Geschwindigkeit nebeneinander.
        let (quality_col, quality_buttons) =
            radio_column("Quality", &QUALITIES.iter().map(|(_, l)| l.to_string()).collect::<Vec<_>>());
        let (speed_col, speed_buttons) = radio_column("Playback speed", &SPEEDS.map(speed_label));
        let menu = gtk::Box::builder().spacing(24).margin_top(8).margin_bottom(8).margin_start(8).margin_end(8).build();
        menu.append(&quality_col);
        menu.append(&speed_col);
        // Upscaling (upscale.rs): wirkt sofort, auch beim laufenden Video.
        let (upscale_col, upscale_buttons) = radio_column("Upscaling", &crate::upscale::MODES.map(|(_, l)| l.to_string()));
        crate::upscale::apply(mpv, crate::upscale::saved());
        for ((mode, _), b) in crate::upscale::MODES.iter().zip(&upscale_buttons) {
            b.set_active(*mode == crate::upscale::saved());
            let mode = *mode;
            b.connect_toggled(move |b| {
                if b.is_active() {
                    crate::upscale::set(mpv, mode);
                }
            });
        }
        menu.append(&upscale_col);
        let settings = gtk::MenuButton::builder()
            .icon_name("boxqu-settings-symbolic")
            .tooltip_text("Settings")
            .direction(gtk::ArrowType::Up)
            .popover(&gtk::Popover::builder().child(&menu).build())
            .focusable(false)
            .css_classes(["flat"])
            .build();

        let saved = saved_quality();
        let on_quality: Rc<RefCell<Option<Box<dyn Fn(u32)>>>> = Rc::default();
        for ((height, _), b) in QUALITIES.iter().zip(&quality_buttons) {
            b.set_active(*height == saved);
            let (height, on_quality) = (*height, on_quality.clone());
            b.connect_toggled(move |b| {
                if !b.is_active() {
                    return;
                }
                let _ = std::fs::create_dir_all(quality_file().parent().unwrap());
                let _ = std::fs::write(quality_file(), height.to_string());
                // Neue Qualität = andere Stufe des Streams: der Player lädt sie an derselben Stelle.
                if let Some(f) = on_quality.borrow().as_ref() {
                    f(height);
                }
            });
        }
        let speed_buttons: Vec<(f64, gtk::CheckButton)> = SPEEDS.into_iter().zip(speed_buttons).collect();
        for (speed, b) in &speed_buttons {
            b.set_active(*speed == 1.0);
            let speed = *speed;
            b.connect_toggled(move |b| {
                if b.is_active() {
                    let _ = mpv.set_property("speed", speed);
                }
            });
        }

        // Jede Gruppe in einer eigenen Bubble.
        let bubble = |widgets: &[&gtk::Widget]| {
            let b = gtk::Box::builder().spacing(2).valign(gtk::Align::Center).css_classes(["bubble"]).build();
            for w in widgets {
                b.append(*w);
            }
            b
        };
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.append(&bubble(&[play.upcast_ref()]));
        buttons.append(&bubble(&[mute.upcast_ref(), vol.upcast_ref()]));
        buttons.append(&bubble(&[time.upcast_ref()]));
        buttons.append(&gtk::Box::builder().hexpand(true).build());
        // Untertitel: Linksklick/Taste c an/aus (verbunden in player.rs, wegen der Einblendung),
        // Rechtsklick = Menü mit Sprache und Aussehen (wie Tubequ).
        let subtitles = icon_button("boxqu-subtitles-symbolic", "Subtitles (c) · right-click for options");
        let sub_prefs = Rc::new(RefCell::new(SubPrefs::load()));
        sub_prefs.borrow().apply(mpv);
        let sub_tracks_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let (size_col, size_buttons) = radio_column("Size", &SUB_SIZES.map(|(_, l)| l.to_string()));
        let (style_col, style_buttons) = radio_column("Background", &["Outline".to_string(), "Box".to_string()]);
        for ((size, _), b) in SUB_SIZES.iter().zip(&size_buttons) {
            b.set_active(*size == sub_prefs.borrow().size);
            let (size, prefs) = (*size, sub_prefs.clone());
            b.connect_toggled(move |b| {
                if b.is_active() {
                    prefs.borrow_mut().size = size;
                    prefs.borrow().apply(mpv);
                    prefs.borrow().save();
                }
            });
        }
        for (boxed, b) in [false, true].into_iter().zip(&style_buttons) {
            b.set_active(boxed == sub_prefs.borrow().boxed);
            let prefs = sub_prefs.clone();
            b.connect_toggled(move |b| {
                if b.is_active() {
                    prefs.borrow_mut().boxed = boxed;
                    prefs.borrow().apply(mpv);
                    prefs.borrow().save();
                }
            });
        }
        let sub_menu_box = gtk::Box::builder().spacing(24).margin_top(8).margin_bottom(8).margin_start(8).margin_end(8).build();
        sub_menu_box.append(&sub_tracks_box);
        sub_menu_box.append(&size_col);
        sub_menu_box.append(&style_col);
        let sub_menu = gtk::Popover::builder().child(&sub_menu_box).position(gtk::PositionType::Top).build();
        sub_menu.set_parent(&subtitles);
        let right = gtk::GestureClick::builder().button(3).build();
        right.connect_pressed({
            let (menu, holder, prefs) = (sub_menu.clone(), sub_tracks_box.clone(), sub_prefs.clone());
            move |_, _, _, _| {
                // Spuren erst beim Öffnen lesen: jedes Video hat andere.
                while let Some(c) = holder.first_child() {
                    holder.remove(&c);
                }
                let tracks = sub_tracks(mpv);
                if tracks.is_empty() {
                    holder.append(&gtk::Label::builder().label("No subtitles for this video").css_classes(["dim-label"]).build());
                } else {
                    let labels: Vec<String> = tracks.iter().map(|(_, l)| l.clone()).collect();
                    let (col, buttons) = radio_column("Language", &labels);
                    let current = mpv.get_property::<i64>("sid").ok();
                    for ((id, lang), b) in tracks.into_iter().zip(&buttons) {
                        b.set_active(Some(id) == current);
                        let prefs = prefs.clone();
                        b.connect_toggled(move |b| {
                            if b.is_active() {
                                let _ = mpv.set_property("sid", id);
                                let mut p = prefs.borrow_mut();
                                (p.lang, p.on) = (lang.clone(), true);
                                p.save();
                            }
                        });
                    }
                    holder.append(&col);
                }
                menu.popup();
            }
        });
        subtitles.add_controller(right);
        buttons.append(&bubble(&[subtitles.upcast_ref(), settings.upcast_ref(), fullscreen.upcast_ref()]));

        // Vorschau über der Timeline (Zeit, ggf. Kapitel), folgt der Maus. Liegt in der Leiste über dem Balken;
        // unsichtbar nimmt sie keinen Platz ein, sichtbar wächst die (unten verankerte) Leiste nach oben.
        let preview_time = gtk::Label::builder().css_classes(["osd", "pill", "numeric"]).halign(gtk::Align::Center).max_width_chars(40).ellipsize(gtk::pango::EllipsizeMode::End).build();
        let preview = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::End)
            .can_target(false)
            .visible(false)
            .build();
        preview.append(&preview_time);
        let chapters: Rc<RefCell<Vec<(f64, String)>>> = Rc::default();

        let bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_start(16)
            .margin_end(16)
            .margin_bottom(14)
            .css_classes(["player-controls"])
            .build();
        seek.add_css_class("timeline");

        // Opening/Ending-Bereiche: eigene Zeichenfläche über dem Balken, genau über dessen Schiene.
        let segments: Rc<RefCell<Vec<Segment>>> = Rc::default();
        let cached_until: Rc<Cell<f64>> = Rc::default();
        let segment_layer = gtk::DrawingArea::builder().can_target(false).build();
        segment_layer.set_draw_func({
            let (segments, cached_until, seek) = (segments.clone(), cached_until.clone(), seek.downgrade());
            move |area, cr, _, _| {
                let Some(seek) = seek.upgrade() else { return };
                let segs = segments.borrow();
                let dur = seek.adjustment().upper();
                // Die Schiene ("trough") ist ein internes Kind des Reglers; ihre Lage bestimmt, wo gezeichnet wird.
                let mut child = seek.first_child();
                while let Some(c) = child.as_ref().filter(|c| c.css_name() != "trough") {
                    child = c.next_sibling();
                }
                let (Some(trough), true) = (child, dur > 1.0) else { return };
                let Some(r) = trough.compute_bounds(area) else { return };
                let (h, y) = (r.height() as f64, r.y() as f64);
                let x_at = |t: f64| r.x() as f64 + r.width() as f64 * (t / dur).clamp(0.0, 1.0);
                // Vorgeladener Bereich (aktuelle Stelle bis Pufferende) heller, wie bei YouTube:
                // dorthin springen geht ohne Warten.
                let (pos, until) = (seek.value(), cached_until.get());
                if until > pos {
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.3);
                    cr.rectangle(x_at(pos), y, x_at(until) - x_at(pos), h);
                    let _ = cr.fill();
                }
                // Dezent in der Schriftfarbe des Themes, keine Signalfarben.
                let c = seek.color();
                for s in segs.iter() {
                    let x0 = r.x() as f64 + r.width() as f64 * (s.start / dur).clamp(0.0, 1.0);
                    let x1 = r.x() as f64 + r.width() as f64 * (s.end / dur).clamp(0.0, 1.0);
                    cr.set_source_rgba(c.red().into(), c.green().into(), c.blue().into(), 0.45);
                    cr.rectangle(x0, y, (x1 - x0).max(2.0), h);
                    let _ = cr.fill();
                }
            }
        });
        let seek_row = gtk::Overlay::builder().child(&seek).build();
        seek_row.add_overlay(&segment_layer);
        bar.append(&seek_row);

        let hover = gtk::EventControllerMotion::new();
        hover.connect_motion({
            let (preview, label, seek, chapters) = (preview.clone(), preview_time.clone(), seek.downgrade(), chapters.clone());
            move |_, x, _| {
                let Some(seek) = seek.upgrade() else { return };
                let dur = seek.adjustment().upper();
                // Zeit unter der Maus: Lage der Schiene ("trough") im Regler.
                let mut child = seek.first_child();
                while let Some(c) = child.as_ref().filter(|c| c.css_name() != "trough") {
                    child = c.next_sibling();
                }
                let (Some(trough), true) = (child, dur > 1.0) else { return };
                let Some(r) = trough.compute_bounds(&seek) else { return };
                let t = ((x - r.x() as f64) / r.width() as f64).clamp(0.0, 1.0) * dur;
                // Zeit und – falls vorhanden – Kapitel an dieser Stelle.
                match chapter_at(&chapters.borrow(), t).filter(|c| !c.is_empty()) {
                    Some(c) => label.set_text(&format!("{} · {c}", fmt_duration(t as u32))),
                    None => label.set_text(&fmt_duration(t as u32)),
                }
                place_preview(&preview, &seek, &trough, x);
                preview.set_visible(true);
            }
        });
        hover.connect_leave({
            let preview = preview.clone();
            move |_| preview.set_visible(false)
        });
        seek.add_controller(hover);
        bar.append(&buttons);

        let widget = gtk::Revealer::builder()
            .child(&bar)
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .reveal_child(true)
            .valign(gtk::Align::End)
            .build();

        let c = Rc::new(Controls {
            widget,
            preview: preview.clone(),
            fullscreen,
            mpv,
            play,
            seek,
            time,
            vol,
            mute,
            settings,
            subtitles,
            sub_menu,
            sub_prefs,
            speed_buttons,
            cursor_target: cursor_target.clone().upcast(),
            hide_timer: RefCell::default(),
            chapters,
            segments,
            segment_layer,
            cached_until,
            on_quality,
            pos: Cell::new(0.0),
            seek_target,
            dur: Cell::new(0.0),
            speed: Cell::new(1.0),
            muted: Cell::new(false),
            fade_gen: Cell::new(0),
            vol_gen: Cell::new(0),
            vol_target: Cell::new(None),
            live: Cell::new(false),
            paused: Cell::new(false),
        });
        c.mute.connect_clicked({
            let c = Rc::downgrade(&c);
            move |_| {
                if let Some(c) = c.upgrade() {
                    c.toggle_mute();
                }
            }
        });
        c
    }

    /// Leiste zeigen und das Ausblenden neu starten (bei Mausbewegung, Taste, Pause …).
    pub fn poke(self: &Rc<Self>) {
        self.widget.set_reveal_child(true);
        self.cursor_target.set_cursor(None);
        self.cursor_target.remove_css_class("idle");
        if let Some(t) = self.hide_timer.take() {
            t.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(HIDE_AFTER, move || {
            let Some(c) = weak.upgrade() else { return };
            c.hide_timer.take(); // Timer ist abgelaufen, nicht mehr entfernen.
            // Maus ruht: Hover-Hervorhebung aus (style.css `.idle`), auch wenn die Leiste sichtbar bleibt.
            c.cursor_target.add_css_class("idle");
            // Pausiert oder bei offenem Einstellungsmenü sichtbar bleiben.
            if !c.paused.get() && !c.menu_open() {
                c.widget.set_reveal_child(false);
                c.cursor_target.set_cursor_from_name(Some("none"));
            }
        });
        self.hide_timer.replace(Some(id));
    }

    /// Ist ein Menü der Leiste offen (Einstellungen, Untertitel)? Dann bleibt die Leiste sichtbar.
    fn menu_open(&self) -> bool {
        self.settings.is_active() || self.sub_menu.is_visible()
    }

    /// Untertitel an/aus (Linksklick, Taste c), wird für die nächsten Videos gemerkt. Beim Einschalten die
    /// gewählte Sprache. `false` = das Video hat keine Untertitel.
    pub fn toggle_subs(&self) -> bool {
        let mut p = self.sub_prefs.borrow_mut();
        if self.mpv.get_property::<String>("sid").is_ok_and(|s| s != "no") {
            let _ = self.mpv.set_property("sid", "no");
            p.on = false;
            p.save();
            return true;
        }
        let Some(id) = pick_sub(&sub_tracks(self.mpv), &p.lang) else { return false };
        p.on = true;
        p.save();
        self.mpv.set_property("sid", id).is_ok()
    }

    /// Neues Video geladen: Untertitel wie zuletzt (aus bzw. in der gewählten Sprache).
    pub fn apply_subs(&self) {
        let p = self.sub_prefs.borrow();
        let id = if p.on { pick_sub(&sub_tracks(self.mpv), &p.lang) } else { None };
        let _ = match id {
            Some(id) => self.mpv.set_property("sid", id),
            None => self.mpv.set_property("sid", "no"),
        };
    }

    /// Sofort ausblenden (Maus hat den Player verlassen). Bei offenem Einstellungsmenü nicht.
    pub fn hide(&self) {
        if self.menu_open() {
            return;
        }
        if let Some(t) = self.hide_timer.take() {
            t.remove();
        }
        self.widget.set_reveal_child(false);
    }

    /// Neues Video: Anzeige zurücksetzen. Bei Livestreams gibt es keinen Fortschrittsbalken.
    pub fn reset(self: &Rc<Self>, live: bool) {
        self.live.set(live);
        self.pos.set(0.0);
        self.dur.set(0.0);
        self.chapters.borrow_mut().clear();
        self.cached_until.set(0.0);
        self.set_segments(vec![]);
        self.seek.clear_marks();
        self.seek.set_visible(!live);
        self.seek.set_value(0.0);
        self.update_time();
        self.poke();
    }

    /// Oberkante der Timeline in den Koordinaten von `target` (z. B. für Knöpfe direkt darüber).
    /// `None`, solange die Leiste nicht angeordnet ist.
    pub fn timeline_top(&self, target: &impl IsA<gtk::Widget>) -> Option<f32> {
        self.seek.compute_bounds(target).map(|r| r.y()).filter(|y| *y > 0.0)
    }

    pub fn connect_quality(&self, f: impl Fn(u32) + 'static) {
        self.on_quality.replace(Some(Box::new(f)));
    }

    pub fn set_segments(&self, segments: Vec<Segment>) {
        self.segments.replace(segments);
        self.segment_layer.queue_draw();
    }

    /// Kapitel aus mpv übernehmen (falls der Stream welche hat) und im Balken markieren.
    pub fn load_chapters(&self) {
        let count = self.mpv.get_property::<i64>("chapter-list/count").unwrap_or(0);
        let chapters: Vec<(f64, String)> = (0..count)
            .filter_map(|i| {
                let start = self.mpv.get_property::<f64>(&format!("chapter-list/{i}/time")).ok()?;
                let title = self.mpv.get_property::<String>(&format!("chapter-list/{i}/title")).unwrap_or_default();
                Some((start, title))
            })
            .collect();
        self.seek.clear_marks();
        for (start, _) in chapters.iter().filter(|(s, _)| *s > 0.0) {
            self.seek.add_mark(*start, gtk::PositionType::Bottom, None);
        }
        self.chapters.replace(chapters);
        self.update_time();
    }

    /// Geschwindigkeit eine Stufe schneller (+1) oder langsamer (-1), wie `>` / `<` bei YouTube.
    pub fn step_speed(&self, dir: i32) {
        let i = SPEEDS.iter().position(|s| *s == self.speed.get()).unwrap_or(3) as i32;
        let next = SPEEDS[(i + dir).clamp(0, SPEEDS.len() as i32 - 1) as usize];
        let _ = self.mpv.set_property("speed", next);
    }

    /// Ton weich aus- bzw. einblenden statt hart umzuschalten (Filter `@fade` in player.rs; die
    /// Lautstärke selbst bleibt dabei unberührt). Knopf, Taste `m` und Miniplayer.
    pub fn toggle_mute(self: &Rc<Self>) {
        let muting = !self.muted.get();
        let generation = self.fade_gen.get() + 1;
        self.fade_gen.set(generation);
        if !muting {
            let _ = self.mpv.command("af-command", &["fade", "volume", "0"]);
            let _ = self.mpv.set_property("mute", false);
        }
        for i in 1..=FADE_STEPS {
            let me = self.clone();
            glib::timeout_add_local_once(FADE_STEP * i, move || {
                if me.fade_gen.get() != generation {
                    return;
                }
                let f = i as f64 / FADE_STEPS as f64;
                let level = if muting { 1.0 - f } else { f };
                let _ = me.mpv.command("af-command", &["fade", "volume", &format!("{level:.3}")]);
                if muting && i == FADE_STEPS {
                    let _ = me.mpv.set_property("mute", true);
                    let _ = me.mpv.command("af-command", &["fade", "volume", "1"]);
                }
            });
        }
    }

    /// Lautstärke um `delta` ändern, in kleinen Schritten statt eines Sprungs (Tasten `+`/`-`).
    pub fn step_volume(self: &Rc<Self>, delta: f64) {
        let from = self.vol_target.get().unwrap_or(self.vol.value());
        let to = (from + delta).clamp(0.0, MAX_VOLUME);
        let start = self.vol.value();
        let generation = self.vol_gen.get() + 1;
        self.vol_gen.set(generation);
        self.vol_target.set(Some(to));
        for i in 1..=FADE_STEPS {
            let me = self.clone();
            glib::timeout_add_local_once(FADE_STEP * i, move || {
                if me.vol_gen.get() != generation {
                    return;
                }
                let v = start + (to - start) * i as f64 / FADE_STEPS as f64;
                let _ = me.mpv.set_property("volume", v.round());
                if i == FADE_STEPS {
                    me.vol_target.set(None);
                }
            });
        }
    }

    /// Wird für jede beobachtete mpv-Eigenschaft aufgerufen, wenn sie sich ändert.
    pub fn update(self: &Rc<Self>, name: &str, data: PropertyData) {
        match (name, data) {
            ("time-pos", PropertyData::Double(t)) => {
                self.pos.set(t);
                // Nach einem Klick in die Timeline: alte Positionen nicht anzeigen, bis mpv am Ziel ist.
                if let Some((target, sent)) = self.seek_target.get() {
                    match seek_state(t, target, sent.map(|s| s.elapsed())) {
                        Seek::Hold => return,
                        Seek::Show => {}
                        Seek::Done => self.seek_target.set(None),
                    }
                }
                // Während der Nutzer den Regler zieht, nicht dazwischenfunken.
                if !self.seek.state_flags().contains(gtk::StateFlags::ACTIVE) {
                    self.seek.set_value(t);
                }
                self.update_time();
            }
            ("demuxer-cache-time", PropertyData::Double(t)) => {
                self.cached_until.set(t);
                self.segment_layer.queue_draw();
            }
            ("duration", PropertyData::Double(d)) => {
                self.dur.set(d);
                self.seek.set_range(0.0, d.max(1.0));
                self.segment_layer.queue_draw();
                self.update_time();
            }
            ("pause", PropertyData::Flag(p)) => {
                self.paused.set(p);
                let (icon, tip) = if p {
                    ("boxqu-play-symbolic", "Play (space)")
                } else {
                    ("boxqu-pause-symbolic", "Pause (space)")
                };
                self.play.set_icon_name(icon);
                self.play.set_tooltip_text(Some(tip));
                self.poke();
            }
            ("speed", PropertyData::Double(s)) => {
                self.speed.set(s);
                if let Some((_, b)) = self.speed_buttons.iter().find(|(v, _)| *v == s) {
                    b.set_active(true);
                }
                self.update_time();
                self.poke();
            }
            // Jede Änderung (Regler, Pfeiltasten) merken.
            ("volume", PropertyData::Double(v)) => {
                self.vol.set_value(v);
                save_volume(v);
            }
            ("mute", PropertyData::Flag(m)) => {
                self.muted.set(m);
                self.mute.set_icon_name(if m { "boxqu-mute-symbolic" } else { "boxqu-volume-symbolic" });
            }
            _ => {}
        }
    }

    fn update_time(&self) {
        // Während an der Timeline gezogen wird, zeigt die Zeit das Ziel (siehe change-value).
        if self.seek.state_flags().contains(gtk::StateFlags::ACTIVE) {
            return;
        }
        let mut parts = vec![if self.live.get() {
            "● LIVE".to_string()
        } else {
            format!("{} / {}", fmt_duration(self.pos.get() as u32), fmt_duration(self.dur.get() as u32))
        }];
        if let Some(c) = chapter_at(&self.chapters.borrow(), self.pos.get()).filter(|c| !c.is_empty()) {
            parts.push(c.to_string());
        }
        if self.speed.get() != 1.0 {
            parts.push(speed_label(self.speed.get()));
        }
        let t = parts.join(" · ");
        if self.time.text() != t {
            self.time.set_text(&t);
        }
    }
}

/// Was die Timeline nach einem Sprung zu `target` mit der gemeldeten Position `pos` macht.
#[derive(Debug, PartialEq)]
enum Seek {
    /// Ziel weiter zeigen (alte Position oder Schlüsselbild vor dem Ziel).
    Hold,
    /// `pos` zeigen, Ziel aber gemerkt lassen: mpv meldet nach dem Sprung erst das Ziel, dann das Schlüsselbild davor.
    Show,
    /// Sprung erledigt, wieder normal.
    Done,
}

/// Noch nicht an mpv gesendet (`since_sent` = `None`): Ziel zeigen. mpv springt zum Schlüsselbild vor dem Ziel
/// (`hr-seek=no`) und spielt von dort hin: bis 1 s hinter dem Ziel nie etwas davor zeigen (spätestens nach 15 s
/// aufgeben). Weit weg (alte Position vor dem Sprung): nur 2 s ignorieren – danach ein anderer Sprung (Tasten).
fn seek_state(pos: f64, target: f64, since_sent: Option<Duration>) -> Seek {
    let Some(d) = since_sent else { return Seek::Hold };
    if pos >= target + 1.0 {
        Seek::Done
    } else if pos >= target {
        Seek::Show
    } else if pos >= target - 10.0 {
        if d > Duration::from_secs(15) { Seek::Done } else { Seek::Hold }
    } else if d > Duration::from_secs(2) {
        Seek::Done
    } else {
        Seek::Hold
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn seek_holds_target() {
        use super::{seek_state, Seek};
        use std::time::Duration;
        let ms = |n| Some(Duration::from_millis(n));
        assert_eq!(seek_state(30.0, 90.0, None), Seek::Hold); // noch nicht gesendet
        assert_eq!(seek_state(30.0, 90.0, ms(100)), Seek::Hold); // alte Position
        assert_eq!(seek_state(90.0, 90.0, ms(0)), Seek::Show); // mpv meldet erst das Ziel …
        assert_eq!(seek_state(83.4, 90.0, ms(50)), Seek::Hold); // … dann das Schlüsselbild davor: Ziel halten
        assert_eq!(seek_state(89.8, 90.0, ms(3500)), Seek::Hold); // knapp davor: würde als 1:29 erscheinen
        assert_eq!(seek_state(90.3, 90.0, ms(3500)), Seek::Show); // am Ziel angekommen
        assert_eq!(seek_state(91.2, 90.0, ms(4500)), Seek::Done); // darüber hinaus: fertig
        assert_eq!(seek_state(30.0, 90.0, ms(2500)), Seek::Done); // weit weg nach 2 s: anderer Sprung
        assert_eq!(seek_state(86.5, 90.0, ms(16000)), Seek::Done); // hängt: aufgeben
    }
    use super::*;

    #[test]
    fn chapters_speed_quality() {
        let ch = vec![(0.0, "Intro".to_string()), (60.0, "Teil 1".to_string()), (300.0, "Ende".to_string())];
        assert_eq!(chapter_at(&ch, 10.0), Some("Intro"));
        assert_eq!(chapter_at(&ch, 60.0), Some("Teil 1"));
        assert_eq!(chapter_at(&ch, 999.0), Some("Ende"));
        assert_eq!(chapter_at(&[], 5.0), None);
        assert_eq!(speed_label(1.5), "1.5×");
        assert_eq!(speed_label(1.0), "Normal");
    }

    #[test]
    fn subtitle_prefs() {
        let t = |v: &[(i64, &str)]| v.iter().map(|(i, l)| (*i, l.to_string())).collect::<Vec<_>>();
        let tracks = t(&[(1, "বাংলা"), (2, "English"), (3, "Français")]);
        assert_eq!(pick_sub(&tracks, "Français"), Some(3));
        assert_eq!(pick_sub(&tracks, "Deutsch"), Some(2), "sonst Englisch");
        assert_eq!(pick_sub(&t(&[(4, "Español")]), "English"), Some(4), "sonst die erste");
        assert_eq!(pick_sub(&[], "English"), None);
        assert_eq!(SubPrefs::parse(""), SubPrefs { lang: "English".into(), size: 38, boxed: false, on: true });
        assert_eq!(SubPrefs::parse("Français\n48\nbox\noff\n"), SubPrefs { lang: "Français".into(), size: 48, boxed: true, on: false });
        assert_eq!(SubPrefs::parse("x\n999\n").size, 38);
    }
}
