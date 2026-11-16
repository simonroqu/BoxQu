//! Schwebendes Suchfenster (wie Spotlight / die Tab-Suche in Zen): abgedunkelter Hintergrund,
//! in der Mitte ein Eingabefeld mit den letzten Suchen. Esc oder Klick daneben schließt es.

use gtk4 as gtk;
use gtk::prelude::*;
use gtk::glib;
use std::rc::Rc;

/// Höchstens so viele Einträge in der Liste.
const MAX_SUGGESTIONS: usize = 8;

pub struct SearchModal {
    /// Liegt als Overlay über dem ganzen Fenster; unsichtbar, solange die Suche zu ist.
    pub widget: gtk::Box,
    entry: gtk::SearchEntry,
    list: gtk::ListBox,
    /// Zuletzt abgeschickte Suche: steht beim nächsten Öffnen wieder im Feld.
    last: std::cell::RefCell<String>,
}

impl SearchModal {
    pub fn new(on_search: impl Fn(String) + 'static) -> Rc<Self> {
        let entry = gtk::SearchEntry::builder().placeholder_text("Search anime").hexpand(true).search_delay(100).build();
        let list = gtk::ListBox::builder().css_classes(["suggestions"]).visible(false).build();
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_top(120)
            .width_request(560)
            .css_classes(["search-card"])
            .build();
        card.append(&entry);
        card.append(&list);

        let widget = gtk::Box::builder().css_classes(["search-backdrop"]).visible(false).build();
        widget.append(&gtk::Box::builder().hexpand(true).build());
        widget.append(&card);
        widget.append(&gtk::Box::builder().hexpand(true).build());

        let modal = Rc::new(SearchModal { widget, entry, list, last: Default::default() });
        let on_search: Rc<dyn Fn(String)> = Rc::new(on_search);

        let submit = {
            let (modal, on_search) = (Rc::downgrade(&modal), on_search.clone());
            move |q: String| {
                let q = q.trim().to_string();
                if let (false, Some(m)) = (q.is_empty(), modal.upgrade()) {
                    crate::lists::add_search(&q);
                    m.last.replace(q.clone());
                    m.close();
                    on_search(q);
                }
            }
        };

        modal.entry.connect_activate({
            let submit = submit.clone();
            move |e| submit(e.text().to_string())
        });
        modal.list.connect_row_activated({
            let submit = submit.clone();
            move |_, row| {
                if let Some(text) = row.child().and_then(|c| c.downcast::<gtk::Label>().ok()) {
                    submit(text.text().to_string());
                }
            }
        });
        modal.entry.connect_stop_search({
            let modal = Rc::downgrade(&modal);
            move |_| {
                if let Some(m) = modal.upgrade() {
                    m.close();
                }
            }
        });

        // Pfeil runter: vom Eingabefeld in die Liste.
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let list = modal.list.downgrade();
            move |_, key, _, _| match (key, list.upgrade().and_then(|l| l.row_at_index(0))) {
                (gtk::gdk::Key::Down, Some(row)) => {
                    row.grab_focus();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        modal.entry.add_controller(keys);

        // ponytail: keine Vorschläge von der Seite (ani-cli kennt keine API dafür). Die letzten Suchen
        // werden beim Tippen gefiltert.
        modal.entry.connect_search_changed({
            let modal = Rc::downgrade(&modal);
            move |e| {
                let Some(m) = modal.upgrade() else { return };
                let q = e.text().trim().to_lowercase();
                m.fill(crate::lists::searches().into_iter().filter(|s| s.to_lowercase().contains(&q)).collect());
            }
        });

        // Klick neben das Suchfeld schließt.
        let click = gtk::GestureClick::new();
        click.connect_pressed({
            let (modal, card) = (Rc::downgrade(&modal), card.downgrade());
            move |g, _, x, y| {
                let (Some(m), Some(card)) = (modal.upgrade(), card.upgrade()) else { return };
                let inside = m.widget.pick(x, y, gtk::PickFlags::DEFAULT).is_some_and(|w| w == card || w.is_ancestor(&card));
                if !inside {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    m.close();
                }
            }
        });
        modal.widget.add_controller(click);
        modal
    }

    fn fill(&self, suggestions: Vec<String>) {
        self.list.remove_all();
        for s in suggestions.iter().take(MAX_SUGGESTIONS) {
            self.list.append(&gtk::Label::builder().label(s).xalign(0.0).build());
        }
        self.list.set_visible(!suggestions.is_empty());
    }

    pub fn is_open(&self) -> bool {
        self.widget.is_visible()
    }

    pub fn open(&self) {
        self.entry.set_text(&self.last.borrow());
        self.fill(crate::lists::searches());
        self.widget.set_visible(true);
        self.entry.grab_focus();
        // Alles markiert: Weitertippen ersetzt die alte Suche.
        self.entry.select_region(0, -1);
    }

    pub fn close(&self) {
        self.widget.set_visible(false);
    }

    /// Zurück zur Startseite: die letzte Suche steht beim nächsten Öffnen nicht mehr im Feld.
    pub fn reset(&self) {
        self.last.replace(String::new());
        self.entry.set_text("");
    }
}
