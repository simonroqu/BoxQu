//! Ansehen-Seite: Video, darunter Titel-Titel (öffnet die Titel-Seite), Episode und
//! Knöpfe für vorherige/nächste Episode. Der Player ist genau so hoch wie das Video.

use crate::show::Play;
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::gdk;
use qu_core::watch::WatchFrame;
use std::cell::RefCell;
use std::rc::Rc;

pub struct WatchPage {
    /// Player-Rahmen aus qu-core (Größe, Strg+Mausrad, Vollbild, Miniplayer); Infos liegen in `frame.below`.
    frame: Rc<WatchFrame>,
    title: gtk::Label,
    episode: gtk::Label,
    pub prev: gtk::Button,
    pub next: gtk::Button,
    /// Was gerade läuft (für Zurück zur Titel-Seite, vorherige/nächste Episode).
    pub current: RefCell<Option<Play>>,
}

/// Rahmen-Methoden (`widget`, `set_mini`, `set_aspect` …) direkt auf der Seite.
impl std::ops::Deref for WatchPage {
    type Target = WatchFrame;
    fn deref(&self) -> &WatchFrame {
        &self.frame
    }
}

impl WatchPage {
    /// Rahmen für die Seitensteuerung (`qu_core::nav::Nav`: Miniplayer, Zoom-Animation).
    pub fn frame(&self) -> Rc<WatchFrame> {
        self.frame.clone()
    }

    pub fn new(player: &impl IsA<gtk::Widget>, on_anime: impl Fn() + 'static) -> Rc<Self> {
        let title = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["title-3"]).build();
        let title_button = gtk::Button::builder()
            .child(&title)
            .css_classes(["flat", "channel-link"])
            .halign(gtk::Align::Start)
            .focusable(false)
            .tooltip_text("All episodes")
            .cursor(&gdk::Cursor::from_name("pointer", None).unwrap())
            .build();
        title_button.connect_clicked(move |_| on_anime());
        let episode = gtk::Label::builder().xalign(0.0).wrap(true).selectable(true).focusable(false).css_classes(["dim-label"]).build();
        let text_col = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).hexpand(true).build();
        text_col.append(&title_button);
        text_col.append(&episode);

        let nav_button = |icon: &str, tip: &str| {
            gtk::Button::builder().icon_name(icon).tooltip_text(tip).focusable(false).valign(gtk::Align::Center).css_classes(["flat"]).build()
        };
        let prev = nav_button("qu-back-symbolic", "Previous episode");
        let next = nav_button("qu-next-symbolic", "Next episode (n)");
        let row = gtk::Box::builder().spacing(8).build();
        row.append(&text_col);
        row.append(&prev);
        row.append(&next);

        let frame = WatchFrame::new(player);
        frame.below.append(&row);
        Rc::new(WatchPage { frame, title, episode, prev, next, current: RefCell::default() })
    }

    pub fn show(&self, p: &Play) {
        let ep = p.episode();
        self.title.set_text(&p.anime.title);
        let name = if ep.title.is_empty() || ep.title == format!("Episode {}", ep.number) { String::new() } else { format!(" · {}", ep.title) };
        // Film (eine Episode): keine Episodenzeile.
        let one = p.episodes.len() == 1;
        self.episode.set_visible(!one);
        self.episode.set_text(&format!("Episode {} of {}{name}", ep.number, p.episodes.len()));
        self.prev.set_visible(!one);
        self.next.set_visible(!one);
        self.prev.set_sensitive(p.prev().is_some());
        self.next.set_sensitive(p.next().is_some());
        self.current.replace(Some(p.clone()));
        self.frame.reset();
    }
}
