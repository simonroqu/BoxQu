//! Titel-Seite: Poster, Titel, Infos, Beschreibung, Sprachfassungen und alle Episoden (Film = eine Episode).

use crate::source::{self, Title, Details, Episode};
use crate::bg;
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// So viele Zeilen Beschreibung sind eingeklappt sichtbar.
const DESCRIPTION_LINES: i32 = 4;
const SHOW_MORE: &str = "Show more";
const SHOW_LESS: &str = "Show less";

/// Eine Episode abspielen: mit der ganzen Liste, damit „nächste Episode“ ohne neue Anfrage geht.
#[derive(Clone)]
pub struct Play {
    pub anime: Title,
    pub episodes: Rc<Vec<Episode>>,
    pub index: usize,
}

impl Play {
    pub fn episode(&self) -> &Episode {
        &self.episodes[self.index]
    }

    pub fn next(&self) -> Option<Play> {
        (self.index + 1 < self.episodes.len()).then(|| Play { index: self.index + 1, ..self.clone() })
    }

    pub fn prev(&self) -> Option<Play> {
        (self.index > 0).then(|| Play { index: self.index - 1, ..self.clone() })
    }
}

fn label(classes: &[&str]) -> gtk::Label {
    gtk::Label::builder().xalign(0.0).wrap(true).selectable(true).focusable(false).css_classes(classes.to_vec()).build()
}

pub struct ShowPage {
    pub widget: gtk::ScrolledWindow,
    poster: gtk::Picture,
    title: gtk::Label,
    meta: gtk::Label,
    description: gtk::Label,
    more: gtk::Button,
    /// Auf die Watchlist setzen/entfernen (Lesezeichen: Kontur = nicht drauf, gefüllt = drauf).
    watchlist: gtk::Button,
    /// Umschalter der Sprachfassungen (Tabs wie Sub/Dub in AniQu), vor dem Lesezeichen.
    languages: gtk::Box,
    flow: gtk::FlowBox,
    /// Lade-Platzhalter (style.css `.skeleton`): Infos + Beschreibung, Episoden-Kacheln, Poster-Fläche.
    ghost_text: gtk::Box,
    ghost_flow: gtk::FlowBox,
    poster_box: gtk::Box,
    status: gtk::Label,
    anime: RefCell<Title>,
    episodes: RefCell<Rc<Vec<Episode>>>,
    generation: Cell<u64>,
    on_play: Rc<dyn Fn(Play)>,
}

impl ShowPage {
    pub fn new(on_play: impl Fn(Play) + 'static) -> Rc<Self> {
        let poster = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .width_request(200)
            .height_request(300)
            .valign(gtk::Align::Start)
            .css_classes(["thumb"])
            .build();
        let title = label(&["title-1"]);
        let meta = label(&["dim-label"]);
        let description = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .lines(DESCRIPTION_LINES)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(90)
            .selectable(true)
            .focusable(false)
            .build();
        let more = gtk::Button::builder().label(SHOW_MORE).halign(gtk::Align::Start).focusable(false).visible(false).css_classes(["flat"]).build();
        let tabs = gtk::Box::builder().spacing(6).margin_top(8).css_classes(["tabs"]).build();
        // Eigene Box mit derselben Klasse, damit die Tabs gleich aussehen (style.css `.tabs > button`).
        let languages = gtk::Box::builder().spacing(6).css_classes(["tabs"]).build();
        tabs.append(&languages);
        let watchlist = gtk::Button::builder().icon_name("boxqu-watchlist-symbolic").focusable(false).valign(gtk::Align::Center).css_classes(["flat"]).build();
        tabs.append(&watchlist);

        // Platzhalter für Infos und Beschreibung, bis die Titel-Seite gelesen ist (Titel ist schon bekannt).
        let ghost_text = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).visible(false).build();
        for (k, n, h) in [(2, 5, 14), (1, 4, 14)] {
            ghost_text.append(&crate::grid::part(k, n, h));
        }
        let desc = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).margin_top(8).build();
        for (k, n) in [(1, 1), (1, 1), (3, 5)] {
            desc.append(&crate::grid::part(k, n, 14));
        }
        ghost_text.append(&desc);

        let text_col = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).hexpand(true).build();
        for w in [title.upcast_ref::<gtk::Widget>(), ghost_text.upcast_ref(), meta.upcast_ref(), description.upcast_ref(), more.upcast_ref(), tabs.upcast_ref()] {
            text_col.append(w);
        }
        // Graue Fläche hinter dem Poster, bis es geladen ist; glänzt, solange die Seite lädt.
        let poster_box = gtk::Box::builder().valign(gtk::Align::Start).overflow(gtk::Overflow::Hidden).css_classes(["thumb-box"]).build();
        poster_box.append(&poster);
        let header = gtk::Box::builder().spacing(24).build();
        header.append(&poster_box);
        header.append(&text_col);

        let flow = gtk::FlowBox::builder()
            .homogeneous(true)
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .max_children_per_line(12)
            .row_spacing(8)
            .column_spacing(8)
            .valign(gtk::Align::Start)
            .build();
        // Platzhalter-Kacheln in Form der Episoden (gleiche Breite wie `fill_episodes`).
        let ghost_flow = gtk::FlowBox::builder()
            .homogeneous(true)
            .selection_mode(gtk::SelectionMode::None)
            .max_children_per_line(12)
            .row_spacing(8)
            .column_spacing(8)
            .valign(gtk::Align::Start)
            .can_target(false)
            .visible(false)
            .build();
        for _ in 0..12 {
            let tile = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).width_request(200).css_classes(["episode"]).build();
            tile.append(&crate::grid::part(1, 2, 16));
            tile.append(&crate::grid::part(3, 4, 12));
            ghost_flow.append(&tile);
        }
        let status = gtk::Label::builder().visible(false).margin_top(32).wrap(true).build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            // Platz für die schwebende Navigation oben links.
            .margin_top(68)
            .margin_start(16)
            .margin_end(16)
            .margin_bottom(32)
            .build();
        content.append(&header);
        content.append(&ghost_flow);
        content.append(&status);
        content.append(&flow);
        let widget = gtk::ScrolledWindow::builder().child(&content).hscrollbar_policy(gtk::PolicyType::Never).build();
        crate::smooth::attach(&widget);

        let page = Rc::new(ShowPage {
            widget,
            poster,
            title,
            meta,
            description,
            more,
            watchlist,
            languages,
            flow,
            ghost_text,
            ghost_flow,
            poster_box,
            status,
            anime: RefCell::default(),
            episodes: RefCell::default(),
            generation: Cell::new(0),
            on_play: Rc::new(on_play),
        });

        page.more.connect_clicked({
            let page = Rc::downgrade(&page);
            move |b| {
                let Some(p) = page.upgrade() else { return };
                let expand = b.label().as_deref() == Some(SHOW_MORE);
                p.description.set_lines(if expand { -1 } else { DESCRIPTION_LINES });
                p.description.set_ellipsize(if expand { gtk::pango::EllipsizeMode::None } else { gtk::pango::EllipsizeMode::End });
                b.set_label(if expand { SHOW_LESS } else { SHOW_MORE });
            }
        });
        page.watchlist.connect_clicked({
            let page = Rc::downgrade(&page);
            move |_| {
                if let Some(p) = page.upgrade() {
                    let on = crate::lists::toggle_watchlist(&p.anime.borrow());
                    p.show_watchlist(on);
                }
            }
        });
        page.flow.connect_child_activated({
            let page = Rc::downgrade(&page);
            move |_, child| {
                let Some(p) = page.upgrade() else { return };
                let play = Play {
                    anime: p.anime.borrow().clone(),
                    episodes: p.episodes.borrow().clone(),
                    index: child.index() as usize,
                };
                (p.on_play)(play);
            }
        });
        // Zurück von einer Episode: Fortschritt auffrischen.
        page.widget.connect_map({
            let page = Rc::downgrade(&page);
            move |_| {
                if let Some(p) = page.upgrade() {
                    p.fill_episodes();
                }
            }
        });
        page
    }

    /// Titel anzeigen; gibt es ihn in der bevorzugten Sprache, gleich diese Fassung.
    pub fn open(self: &Rc<Self>, a: Title) {
        self.open_as(a, true);
    }

    /// Titel anzeigen: Bekanntes sofort, Details und Episoden im Hintergrund. `follow` = zur
    /// bevorzugten Sprachfassung wechseln (nicht nach einem Klick auf einen Sprach-Tab).
    fn open_as(self: &Rc<Self>, a: Title, follow: bool) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let same = self.anime.borrow().id == a.id;
        if !same {
            self.widget.vadjustment().set_value(0.0);
            self.episodes.replace(Rc::default());
            self.poster.set_paintable(None::<&gdk::Paintable>);
            self.description.set_text("");
            self.more.set_visible(false);
            self.set_anime(&a);
            self.fill_episodes();
            self.set_loading(true);
        }
        self.status.set_visible(false);
        self.show_watchlist(crate::lists::on_watchlist(&a.id));
        self.anime.replace(a.clone());

        let page = self.clone();
        let busy = crate::Busy::new();
        glib::spawn_future_local(async move {
            let id = a.id.clone();
            // Beide Anfragen gleichzeitig.
            let (details, episodes) = bg(async move {
                let d = tokio::spawn(source::details(id.clone()));
                let e = source::episodes(id).await;
                (d.await.unwrap_or_else(|e| Err(e.to_string())), e)
            })
            .await;
            drop(busy);
            if page.generation.get() != generation {
                return;
            }
            // Andere Fassung bevorzugt (z. B. Original statt Hindi): die öffnen.
            if let Some(l) = details.as_ref().ok().filter(|_| follow).and_then(|d| crate::lists::preferred(&d.languages)).filter(|l| l.id != a.id) {
                return page.open_as(Title { id: l.id.clone(), ..a }, false);
            }
            page.set_loading(false);
            match details {
                Ok(d) => page.set_details(d),
                Err(e) => eprintln!("Titel-Seite nicht gelesen: {e}"),
            }
            match episodes {
                Ok(list) if list.is_empty() => page.message("No episodes yet"),
                Ok(list) => {
                    page.episodes.replace(Rc::new(list));
                    page.fill_episodes();
                }
                Err(e) if page.episodes.borrow().is_empty() => page.message(&format!("Error: {e}")),
                Err(e) => eprintln!("Episoden nicht neu geladen: {e}"),
            }
        });
    }

    /// Platzhalter an/aus. Die Episoden-Platzhalter nur, solange noch keine Episoden da sind.
    fn set_loading(&self, on: bool) {
        self.ghost_text.set_visible(on);
        self.ghost_flow.set_visible(on && self.episodes.borrow().is_empty());
        if on { self.poster_box.add_css_class("skeleton") } else { self.poster_box.remove_css_class("skeleton") }
    }

    fn show_watchlist(&self, on: bool) {
        self.watchlist.set_icon_name(if on { "boxqu-watchlisted-symbolic" } else { "boxqu-watchlist-symbolic" });
        self.watchlist.set_tooltip_text(Some(if on { "Remove from Watchlist" } else { "Add to Watchlist" }));
    }

    pub fn reload(self: &Rc<Self>) {
        let a = self.anime.borrow().clone();
        self.open_as(a, false);
    }

    fn message(&self, text: &str) {
        self.status.set_text(text);
        self.status.set_visible(true);
    }

    fn set_anime(&self, a: &Title) {
        self.title.set_text(&a.title);
        if self.poster.paintable().is_none() {
            crate::grid::load_poster(&self.poster, a.poster.clone());
        }
    }

    fn set_details(self: &Rc<Self>, d: Details) {
        // Bekanntes (z. B. aus der Suche) behalten, wenn die Seite etwas nicht liefert.
        let old = self.anime.borrow().clone();
        let a = Title {
            poster: d.item.poster.or(old.poster),
            kind: d.item.kind.or(old.kind),
            duration: d.item.duration.or(old.duration),
            title: if d.item.title.is_empty() { old.title } else { d.item.title },
            id: old.id,
        };
        self.set_anime(&a);
        let meta: Vec<String> = [a.kind.clone(), a.duration.clone()].into_iter().flatten().chain(d.info).collect();
        let mut lines = vec![meta.join(" · ")];
        if !d.genres.is_empty() {
            lines.push(d.genres.join(", "));
        }
        self.meta.set_text(&lines.join("\n"));
        self.description.set_text(&d.description);
        self.more.set_visible(d.description.len() > 300);
        self.set_languages(&d.languages, &a);
        self.anime.replace(a);
    }

    /// Ein Tab je Sprachfassung; Klick merkt die Sprache für alle Titel und öffnet diese Fassung.
    fn set_languages(self: &Rc<Self>, list: &[source::Language], a: &Title) {
        while let Some(c) = self.languages.first_child() {
            self.languages.remove(&c);
        }
        self.languages.set_visible(list.len() > 1);
        if list.len() < 2 {
            return;
        }
        let mut first: Option<gtk::ToggleButton> = None;
        for l in list {
            let tab = gtk::ToggleButton::builder().label(&l.name).active(l.id == a.id).focusable(false).build();
            if l.original {
                tab.set_tooltip_text(Some(&format!("Original audio ({})", l.code.to_uppercase())));
            }
            tab.set_group(first.as_ref());
            first.get_or_insert(tab.clone());
            let (page, l, a) = (Rc::downgrade(self), l.clone(), a.clone());
            tab.connect_clicked(move |t| {
                let Some(p) = page.upgrade() else { return };
                if !t.is_active() || p.anime.borrow().id == l.id {
                    return;
                }
                crate::lists::set_language(if l.original { "original" } else { &l.code });
                p.open_as(Title { id: l.id.clone(), ..a.clone() }, false);
            });
            self.languages.append(&tab);
        }
    }

    /// Episoden-Kacheln: Nummer, Titel, Fortschritt. Ein Film hat eine Kachel „Play“.
    fn fill_episodes(&self) {
        self.flow.remove_all();
        let a = self.anime.borrow().clone();
        for ep in self.episodes.borrow().iter() {
            let tile = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).width_request(200).css_classes(["episode"]).build();
            tile.append(&gtk::Label::builder().label(if self.episodes.borrow().len() == 1 { "Play".to_string() } else { format!("Episode {}", ep.number) }).xalign(0.0).css_classes(["heading"]).build());
            let title = gtk::Label::builder()
                .label(&ep.title)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(1)
                .hexpand(true)
                .css_classes(["dim-label", "caption"])
                .build();
            tile.append(&title);
            let entry = crate::progress::get(&a.id, ep.number);
            let bar = gtk::ProgressBar::builder().fraction(entry.as_ref().map_or(0.0, |e| e.fraction())).margin_top(4).css_classes(["watched"]).build();
            bar.set_opacity(if entry.is_some() { 1.0 } else { 0.0 });
            tile.append(&bar);
            crate::fly::register(&ep.id, &tile);
            if entry.is_some_and(|e| e.finished()) {
                tile.set_opacity(0.5);
            }
            let child = gtk::FlowBoxChild::builder().child(&tile).build();
            child.set_cursor_from_name(Some("pointer"));
            self.flow.append(&child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_and_prev() {
        let eps: Rc<Vec<Episode>> =
            Rc::new((1..=2).map(|n| Episode { id: n.to_string(), number: n, title: String::new() }).collect());
        let p = Play { anime: Title::default(), episodes: eps, index: 0 };
        let n = p.next().expect("Episode 2");
        assert!(n.next().is_none());
        assert_eq!(n.prev().map(|p| p.index), Some(0));
    }
}
