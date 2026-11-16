//! Raster aus Titel-Karten (Poster). Startseite („Continue watching“, Watchlist, je Quelle ein Bereich),
//! ganze Quelle und Suche.

use crate::source::{self, Title};
use crate::bg;
use crate::progress::{self, Resume};
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub fn fmt_duration(s: u32) -> String {
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Kleinste Kartenbreite plus Abstand, um aus der verfügbaren Breite die Spaltenzahl zu berechnen.
/// Die Karten wachsen dann mit, bis die Reihe gefüllt ist.
const CARD_WIDTH: i32 = 170;
/// Höchstens so viele Karten pro Reihe. Hoch genug, dass Poster im Vollbild nicht riesig werden
/// (Kartenbreite bleibt so etwa zwischen 170 und 250 px).
const MAX_COLUMNS: i32 = 12;
const COLUMN_SPACING: i32 = 16;
/// Höchstens so viele Titel in „Continue watching“.
const SHELF_MAX: usize = 30;
/// Pro Quelle auf der Startseite etwa so viele Titel (auf volle Reihen gerundet).
const SOURCE_SHOWN: usize = 12;
/// So viele pro Quelle laden: reicht für volle Reihen bei jeder Spaltenzahl, auch ohne Watchlist-Titel.
const SOURCE_FETCH: usize = 30;
/// „Show all“ einer Quelle.
const SOURCE_ALL: usize = 500;

/// Wie viele von `available` Titeln zeigen: `SOURCE_SHOWN` auf volle Reihen aufgerundet;
/// reicht der Vorrat nicht, nur die vollen Reihen (mindestens eine, auch wenn unvollständig).
fn shown_count(available: usize, cols: usize) -> usize {
    let cols = cols.max(1);
    let want = SOURCE_SHOWN.div_ceil(cols) * cols;
    if available >= want { want } else { (available / cols * cols).max(available.min(cols)) }
}

/// Abstand oben bis zur ersten Karte (unter der schwebenden Navigation).
const TOP_MARGIN: i32 = 56;

/// Mit Strg+Mausrad wählbare Spaltenzahl (Poster: weniger als 4 wären riesig).
const MIN_WANT: usize = 4;
const MAX_WANT: usize = 12;
/// Gewählte Spalten werden nur so weit gezeigt, wie Karten dieser Breite noch passen.
const MIN_CARD_WIDTH: i32 = 120;

/// Spalten für eine Breite: ohne Wahl so viele Karten wie passen, höchstens `MAX_COLUMNS`;
/// mit Strg+Mausrad gewählt (`want`) diese Zahl, solange Karten ≥ `MIN_CARD_WIDTH` passen.
fn columns(width: i32, want: Option<usize>) -> usize {
    let fit = |card: i32| ((width + COLUMN_SPACING) / (card + COLUMN_SPACING + 8)).max(1) as usize;
    match want {
        Some(n) => n.min(fit(MIN_CARD_WIDTH)),
        None => fit(CARD_WIDTH).min(MAX_COLUMNS as usize),
    }
}

/// Eine Spalte mehr (`dy` < 0, Mausrad hoch) oder weniger, in `MIN_WANT..=MAX_WANT`.
fn step_columns(current: usize, dy: f64) -> usize {
    let n = if dy < 0.0 { current + 1 } else { current.saturating_sub(1) };
    n.clamp(MIN_WANT, MAX_WANT)
}

/// Poster-Höhe (Hochformat 2:3) zur Kartenbreite.
fn poster_height(width: i32) -> i32 {
    width * 3 / 2
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(1) // Breite kommt von der Karte, nicht vom Text
        .hexpand(true)
        .css_classes(classes.to_vec())
        .build()
}

/// Poster laden (aus dem Cache oder aus dem Netz) und anzeigen.
pub fn load_poster(pic: &gtk::Picture, url: Option<String>) {
    let Some(url) = url else { return };
    // Unsichtbar bis zum Laden, dann einblenden (style.css `.fade`).
    pic.add_css_class("fade");
    pic.remove_css_class("loaded");
    let pic = pic.downgrade();
    glib::spawn_future_local(async move {
        let Ok(bytes) = bg(source::poster(url)).await else { return };
        let Ok(tex) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)) else { return };
        if let Some(pic) = pic.upgrade() {
            pic.set_paintable(Some(&tex));
            pic.add_css_class("loaded");
        }
    });
}

/// Lade-Platzhalter in Form einer Karte: Poster, Titel, Infos (style.css `.skeleton`). Gibt auch den
/// Poster-Platzhalter zurück: seine Höhe folgt wie bei echten Karten der Spaltenbreite (`fit`).
fn ghost_card(width: i32) -> (gtk::Box, gtk::Box) {
    let b = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).build();
    let poster = gtk::Box::builder().height_request(poster_height(width)).css_classes(["skeleton"]).build();
    b.append(&poster);
    b.append(&part(4, 5, 16));
    b.append(&part(1, 2, 12));
    (b, poster)
}

/// Grauer Balken über `k` von `n` gleich breiten Teilen der Zeile – ohne feste Pixelbreite,
/// die dem Fenster eine Mindestbreite aufzwänge.
pub fn part(k: i32, n: i32, height: i32) -> gtk::Grid {
    let g = gtk::Grid::builder().column_homogeneous(true).build();
    g.attach(&gtk::Box::builder().height_request(height).css_classes(["skeleton"]).build(), 0, 0, k, 1);
    // Jede freie Spalte braucht ein Kind, sonst zählt sie bei gleich breiten Spalten nicht mit.
    for i in k..n {
        g.attach(&gtk::Box::new(gtk::Orientation::Horizontal, 0), i, 0, 1, 1);
    }
    g
}

/// Was eine Karte beim Anklicken öffnet.
#[derive(Clone)]
enum Item {
    Title(Title),
    Resume(Resume),
}

fn card(item: &Item, width: i32) -> (gtk::Box, gtk::Picture) {
    let (a, meta, fraction) = match item {
        Item::Title(a) => {
            let meta: Vec<String> = [a.kind.clone(), a.duration.clone()].into_iter().flatten().collect();
            (a, meta.join(" · "), None)
        }
        Item::Resume(r) => (&r.anime, if r.total > 1 { format!("Episode {}", r.episode) } else { String::new() }, Some(r.fraction)),
    };
    let pic = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .height_request(poster_height(width))
        .css_classes(["thumb"])
        .build();
    load_poster(&pic, a.poster.clone());
    // Nur „Continue watching“ startet direkt eine Episode: Startpunkt der Öffnen-Animation.
    if matches!(item, Item::Resume(_)) {
        crate::fly::register(&a.id, &pic);
    }
    // Grauer Platzhalter in Bildgröße, bis das Bild da ist; schneidet auch den Hover-Zoom ab.
    let thumb = gtk::Overlay::builder().child(&pic).overflow(gtk::Overflow::Hidden).css_classes(["thumb-box"]).build();

    if let Some(f) = fraction.filter(|f| *f > 0.0) {
        let bar = gtk::ProgressBar::builder().valign(gtk::Align::End).fraction(f).css_classes(["watched"]).build();
        thumb.add_overlay(&bar);
    }

    // Keine Mindestbreite: die Spaltenzahl folgt aus `columns`, die Karte füllt ihre Spalte.
    let b = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).build();
    card_menu(&b, &a.id, matches!(item, Item::Resume(_)));
    b.append(&thumb);
    b.append(&label(&a.title, &["heading"]));
    if !meta.is_empty() {
        b.append(&label(&meta, &["dim-label", "caption"]));
    }
    (b, pic)
}

/// Rechtsklick: Watchlist an/aus, bei „Continue watching“ zusätzlich „Watched“.
/// Die Aktionen (`card.*`) hängen am Raster, siehe `Grid::new`.
fn card_menu(card: &gtk::Box, id: &str, resume: bool) {
    let click = gtk::GestureClick::builder().button(3).build();
    let id = id.to_string();
    click.connect_pressed(move |g, _, x, y| {
        let Some(card) = g.widget() else { return };
        g.set_state(gtk::EventSequenceState::Claimed);
        let menu = gtk::gio::Menu::new();
        let label = if crate::lists::on_watchlist(&id) { "Remove from Watchlist" } else { "Add to Watchlist" };
        menu.append(Some(label), Some(&gtk::gio::Action::print_detailed_name("card.watchlist", Some(&id.to_variant()))));
        if resume {
            menu.append(Some("Watched"), Some(&gtk::gio::Action::print_detailed_name("card.watched", Some(&id.to_variant()))));
        }
        let pop = gtk::PopoverMenu::from_model(Some(&menu));
        pop.set_parent(&card);
        pop.set_has_arrow(false);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        // Nach dem Schließen wieder vom Karten-Widget lösen.
        pop.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        pop.popup();
    });
    card.add_controller(click);
}

/// Überschrift plus Karten-Reihe(n).
struct Section {
    title: gtk::Label,
    flow: gtk::FlowBox,
    items: RefCell<Vec<Item>>,
    pics: RefCell<Vec<gtk::Picture>>,
    /// Alle Titel (nur Quellen-Bereiche): angezeigt ohne Watchlist-Titel, auf volle Reihen gekürzt.
    all: RefCell<Vec<Title>>,
}

impl Section {
    fn new(title: &str) -> Rc<Self> {
        let flow = gtk::FlowBox::builder()
            .homogeneous(true)
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .row_spacing(24)
            .column_spacing(COLUMN_SPACING as u32)
            .valign(gtk::Align::Start)
            .build();
        let title = gtk::Label::builder().label(title).xalign(0.0).margin_bottom(12).css_classes(["title-3"]).visible(!title.is_empty()).build();
        Rc::new(Section { title, flow, items: RefCell::default(), pics: RefCell::default(), all: RefCell::default() })
    }

    fn set(&self, items: Vec<Item>, width: i32) {
        self.flow.remove_all();
        self.pics.borrow_mut().clear();
        for item in &items {
            let (card, pic) = card(item, width);
            self.pics.borrow_mut().push(pic);
            let child = gtk::FlowBoxChild::builder().child(&card).cursor(&gdk::Cursor::from_name("pointer", None).unwrap()).build();
            self.flow.append(&child);
        }
        let empty = items.is_empty();
        self.items.replace(items);
        self.flow.set_visible(!empty);
        if !self.title.label().is_empty() {
            self.title.set_visible(!empty);
        }
    }

    /// Quellen-Bereich: Titel ohne Watchlist, `SOURCE_SHOWN` auf volle Reihen.
    fn fill(&self, cols: usize, width: i32) {
        let list: Vec<Title> = self.all.borrow().iter().filter(|a| !crate::lists::on_watchlist(&a.id)).cloned().collect();
        let n = shown_count(list.len(), cols);
        self.set(list.into_iter().take(n).map(Item::Title).collect(), width);
    }
}

/// Was das Raster zeigt (für Reload).
#[derive(Clone)]
enum View {
    Home,
    Search(String),
    Source(source::Source),
}

pub struct Grid {
    pub widget: gtk::Overlay,
    scroll: gtk::ScrolledWindow,
    shelf: Rc<Section>,
    watchlist: Rc<Section>,
    results: Rc<Section>,
    /// Je eingetragene Quelle ein Bereich (Startseite), in `sources_box`.
    sources: RefCell<Vec<(source::Source, Rc<Section>)>>,
    sources_box: gtk::Box,
    status: gtk::Label,
    /// Graue Platzhalter-Karten beim Laden; ihre Poster-Höhen folgen der Spaltenbreite.
    ghost: gtk::FlowBox,
    ghost_posters: RefCell<Vec<gtk::Box>>,
    view: RefCell<View>,
    column_width: Cell<i32>,
    /// Zuletzt gemessene Breite (ohne Seitenränder) und aktuelle Spaltenzahl.
    width: Cell<i32>,
    columns: Cell<usize>,
    /// Mit Strg+Mausrad gewählte Spaltenzahl, gespeichert in `~/.config/boxqu/columns`.
    want: Cell<Option<usize>>,
    /// Wird bei jedem neuen Inhalt erhöht, damit verspätete Antworten alter Suchen verworfen werden.
    generation: Cell<u64>,
    on_open: Rc<dyn Fn(Title)>,
    on_resume: Rc<dyn Fn(Resume)>,
}

impl Grid {
    pub fn new(on_open: impl Fn(Title) + 'static, on_resume: impl Fn(Resume) + 'static) -> Rc<Self> {
        let shelf = Section::new("Continue watching");
        let watchlist = Section::new("Watchlist");
        let results = Section::new("");
        // Laden: graue Platzhalter-Karten. Das Label zeigt nur Fehler oder „keine Ergebnisse“.
        let status = gtk::Label::builder().visible(false).margin_top(48).wrap(true).build();
        let ghost = gtk::FlowBox::builder()
            .homogeneous(true)
            .selection_mode(gtk::SelectionMode::None)
            .row_spacing(24)
            .column_spacing(COLUMN_SPACING as u32)
            .valign(gtk::Align::Start)
            .can_target(false)
            .visible(false)
            .build();

        // Unsichtbare Messfläche über der ganzen Seite: meldet die Fensterbreite, daraus folgt die Spaltenzahl.
        let sizer = gtk::DrawingArea::builder().can_target(false).build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_top(TOP_MARGIN)
            .margin_start(16)
            .margin_end(16)
            .margin_bottom(32)
            .build();
        content.append(&status);
        // „Continue watching“ ganz oben, dann Watchlist, darunter die Quellen.
        for s in [&shelf, &watchlist] {
            content.append(&s.title);
            content.append(&s.flow);
        }
        let sources_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
        content.append(&sources_box);
        content.append(&ghost);
        content.append(&results.title);
        content.append(&results.flow);
        let scroll = gtk::ScrolledWindow::builder().child(&content).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
        crate::smooth::attach(&scroll);
        let widget = gtk::Overlay::builder().child(&scroll).build();
        widget.add_overlay(&sizer);

        let grid = Rc::new(Grid {
            widget,
            scroll,
            shelf,
            watchlist,
            results,
            sources: RefCell::default(),
            sources_box,
            status,
            ghost,
            ghost_posters: RefCell::default(),
            view: RefCell::new(View::Home),
            column_width: Cell::new(CARD_WIDTH),
            width: Cell::new(0),
            columns: Cell::new(0),
            want: Cell::new(
                std::fs::read_to_string(crate::controls::config_file("columns")).ok().and_then(|s| s.trim().parse().ok()),
            ),
            generation: Cell::new(0),
            on_open: Rc::new(on_open),
            on_resume: Rc::new(on_resume),
        });
        for s in [&grid.shelf, &grid.watchlist, &grid.results] {
            grid.connect_section(s);
        }

        sizer.connect_resize({
            let grid = Rc::downgrade(&grid);
            move |_, page_width, _| {
                if let Some(grid) = grid.upgrade() {
                    // Abzüglich der Seitenränder des Inhalts (je 16 px).
                    grid.width.set(page_width - 32);
                    grid.fit();
                }
            }
        });

        // Strg+Mausrad: eine Spalte mehr/weniger (vor dem weichen Scrollen der Seite).
        let zoom_ctl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        zoom_ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
        zoom_ctl.connect_scroll({
            let grid = Rc::downgrade(&grid);
            move |c, _, dy| {
                let Some(grid) = grid.upgrade() else { return glib::Propagation::Proceed };
                if !c.current_event_state().contains(gdk::ModifierType::CONTROL_MASK) {
                    return glib::Propagation::Proceed;
                }
                // ponytail: Touchpad (Pixel statt Rasten) wird nur geschluckt; aufsummieren, falls gewünscht.
                if c.unit() == gdk::ScrollUnit::Wheel && dy != 0.0 {
                    let n = step_columns(grid.columns.get(), dy);
                    grid.want.set(Some(n));
                    let f = crate::controls::config_file("columns");
                    let _ = std::fs::create_dir_all(f.parent().unwrap());
                    let _ = std::fs::write(f, n.to_string());
                    grid.fit();
                }
                glib::Propagation::Stop
            }
        });
        grid.widget.add_controller(zoom_ctl);

        // Aktionen für das Karten-Menü (`card_menu`) und die Quellen-Überschriften; Parameter = ID bzw. Adresse.
        let actions = gtk::gio::SimpleActionGroup::new();
        let add = |name: &str, f: fn(&Rc<Grid>, &str)| {
            let a = gtk::gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
            let grid = Rc::downgrade(&grid);
            a.connect_activate(move |_, p| {
                if let (Some(g), Some(id)) = (grid.upgrade(), p.and_then(|p| p.get::<String>())) {
                    f(&g, &id);
                }
            });
            actions.add_action(&a);
        };
        add("watchlist", |g, id| {
            if let Some(a) = g.find(id) {
                crate::lists::toggle_watchlist(&a);
                if g.is_home() {
                    g.fill_shelf();
                }
            }
        });
        add("watched", |g, id| {
            progress::hide(id);
            g.fill_shelf();
        });
        add("remove-source", |g, url| {
            source::remove_source(url);
            g.show_home();
        });
        grid.widget.insert_action_group("card", Some(&actions));

        // Zurück auf der Startseite (z. B. nach einer Episode): „Continue watching“ auffrischen.
        grid.widget.connect_map({
            let grid = Rc::downgrade(&grid);
            move |_| {
                if let Some(grid) = grid.upgrade().filter(|g| g.is_home()) {
                    let adj = grid.scroll.vadjustment();
                    let pos = adj.value();
                    grid.fill_shelf();
                    glib::idle_add_local_once(move || adj.set_value(pos));
                }
            }
        });
        grid
    }

    /// Klick auf eine Karte: Titel-Seite bzw. Episode fortsetzen.
    fn connect_section(&self, s: &Rc<Section>) {
        let (s2, on_open, on_resume) = (Rc::downgrade(s), self.on_open.clone(), self.on_resume.clone());
        s.flow.connect_child_activated(move |_, child| {
            let Some(s) = s2.upgrade() else { return };
            let item = s.items.borrow().get(child.index() as usize).cloned();
            match item {
                Some(Item::Title(a)) => on_open(a),
                Some(Item::Resume(r)) => on_resume(r),
                None => {}
            }
        });
    }

    fn all_sections(&self) -> Vec<Rc<Section>> {
        let mut v = vec![self.shelf.clone(), self.watchlist.clone(), self.results.clone()];
        v.extend(self.sources.borrow().iter().map(|(_, s)| s.clone()));
        v
    }

    /// Spaltenzahl und Posterhöhen aus der gemessenen Breite und der Spaltenwahl.
    fn fit(self: &Rc<Self>) {
        let width = self.width.get();
        if width <= 0 {
            return;
        }
        let cols = columns(width, self.want.get());
        if cols != self.columns.get() {
            self.columns.set(cols);
            // Quellen füllen immer ganze Reihen: bei anderer Spaltenzahl neu zuschneiden.
            if self.is_home() {
                self.fill_sources();
            }
        }
        let col_width = (width - (cols as i32 - 1) * COLUMN_SPACING) / cols as i32;
        if (col_width - self.column_width.get()).abs() > 4 {
            self.column_width.set(col_width);
            for s in self.all_sections() {
                for pic in s.pics.borrow().iter() {
                    pic.set_height_request(poster_height(col_width));
                }
            }
            for p in self.ghost_posters.borrow().iter() {
                p.set_height_request(poster_height(col_width));
            }
        }
        // Feste Spaltenzahl (FlowBox würde sonst nach der natürlichen Bildbreite umbrechen).
        // Nicht mitten in der Größenberechnung umbauen, sondern direkt danach.
        let grid = self.clone();
        glib::idle_add_local_once(move || grid.set_columns(cols));
    }

    fn set_columns(&self, cols: usize) {
        let flows = self.all_sections().into_iter().map(|s| s.flow.clone()).chain([self.ghost.clone()]);
        for flow in flows {
            flow.set_min_children_per_line(cols as u32);
            flow.set_max_children_per_line(cols as u32);
        }
    }

    fn fill_shelf(&self) {
        let items: Vec<Item> = progress::continue_watching(SHELF_MAX).into_iter().map(Item::Resume).collect();
        let marked: Vec<Item> = crate::lists::watchlist().into_iter().map(Item::Title).collect();
        // Abstand zum Abschnitt darüber nur, wenn einer darüber steht.
        self.watchlist.title.set_margin_top(if items.is_empty() { 0 } else { 32 });
        self.shelf.set(items, self.column_width.get());
        self.watchlist.set(marked, self.column_width.get());
        self.fill_sources();
    }

    fn fill_sources(&self) {
        let mut above = self.shelf.flow.is_visible() || self.watchlist.flow.is_visible();
        for (_, s) in self.sources.borrow().iter() {
            s.title.set_margin_top(if above { 32 } else { 0 });
            s.fill(self.columns.get(), self.column_width.get());
            above |= s.flow.is_visible();
        }
    }

    /// Angezeigter Titel mit dieser ID (für das Karten-Menü).
    fn find(&self, id: &str) -> Option<Title> {
        self.all_sections().iter().find_map(|s| {
            s.items.borrow().iter().find_map(|i| match i {
                Item::Title(a) if a.id == id => Some(a.clone()),
                Item::Resume(r) if r.anime.id == id => Some(r.anime.clone()),
                _ => None,
            })
        })
    }

    fn begin(&self, view: View) -> u64 {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.view.replace(view);
        self.status.set_visible(false);
        self.scroll.vadjustment().set_value(0.0);
        generation
    }

    /// Platzhalter-Karten zeigen (an) oder entfernen (aus). So viele Reihen, dass die sichtbare Höhe
    /// gefüllt ist; beim App-Start (noch ohne Größe) 4 Reihen bei 6 Spalten.
    fn set_ghost(&self, on: bool) {
        self.ghost.remove_all();
        self.ghost_posters.borrow_mut().clear();
        self.ghost.set_visible(on);
        if !on {
            return;
        }
        let cols = if self.columns.get() > 0 { self.columns.get() } else { 6 };
        let width = self.column_width.get();
        // ponytail: ~60 px Text unter dem Poster + Reihenabstand geschätzt.
        let h = self.scroll.height();
        let rows = if h > 0 { (h / (poster_height(width) + 60).max(1)) as usize + 1 } else { 4 };
        self.ghost.set_min_children_per_line(cols as u32);
        self.ghost.set_max_children_per_line(cols as u32);
        for _ in 0..rows.max(1) * cols {
            let (card, poster) = ghost_card(width);
            self.ghost.append(&card);
            self.ghost_posters.borrow_mut().push(poster);
        }
    }

    /// Hinweis oder Fehler über dem Inhalt (verschwindet beim nächsten Laden).
    pub fn message(&self, text: &str) {
        self.set_ghost(false);
        self.status.set_text(text);
        self.status.set_visible(true);
    }

    /// Überschrift eines Quellen-Bereichs: Klick = alle Titel, Rechtsklick = Quelle entfernen.
    fn source_header(self: &Rc<Self>, src: &source::Source, s: &Section) {
        s.title.set_label(&format!("{}  ›", src.name));
        s.title.set_halign(gtk::Align::Start);
        s.title.set_cursor_from_name(Some("pointer"));
        s.title.set_tooltip_text(Some("Show all"));
        let click = gtk::GestureClick::builder().button(0).build();
        let (grid, src) = (Rc::downgrade(self), src.clone());
        click.connect_pressed(move |g, _, x, y| {
            let Some(grid) = grid.upgrade() else { return };
            g.set_state(gtk::EventSequenceState::Claimed);
            // Bereiche sind fest (MovieBox-Startseite): kein Rechtsklick-Menü zum Entfernen.
            if g.current_button() != 3 {
                return grid.show_source(src.clone());
            }
            if source::sources().iter().any(|s| s.url == src.url) {
                return;
            }
            let Some(label) = g.widget() else { return };
            let menu = gtk::gio::Menu::new();
            menu.append(Some("Remove source"), Some(&gtk::gio::Action::print_detailed_name("card.remove-source", Some(&src.url.to_variant()))));
            let pop = gtk::PopoverMenu::from_model(Some(&menu));
            pop.set_parent(&label);
            pop.set_has_arrow(false);
            pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            pop.connect_closed(|p| {
                let p = p.clone();
                glib::idle_add_local_once(move || p.unparent());
            });
            pop.popup();
        });
        s.title.add_controller(click);
    }

    /// Startseite: je Quelle die zuletzt geladene Liste sofort, die neue ersetzt sie; darüber „Continue watching“.
    pub fn show_home(self: &Rc<Self>) {
        let generation = self.begin(View::Home);
        self.results.set(vec![], self.column_width.get());
        // Bereiche neu aufbauen (Quellen können hinzugekommen oder entfernt worden sein).
        while let Some(c) = self.sources_box.first_child() {
            self.sources_box.remove(&c);
        }
        let list = source::sources();
        let sections: Vec<(source::Source, Rc<Section>)> = list.into_iter().map(|src| (src.clone(), Section::new(&src.name))).collect();
        for (src, s) in &sections {
            self.source_header(src, s);
            self.connect_section(s);
            s.all.replace(source::saved_titles(&src.url));
            self.sources_box.append(&s.title);
            self.sources_box.append(&s.flow);
        }
        self.sources.replace(sections.clone());
        self.set_columns(self.columns.get().max(1));
        self.fill_shelf();
        if sections.is_empty() {
            self.message("No sources yet. Paste a playlist (M3U), Internet Archive collection or video link with Ctrl+V.");
            return;
        }
        self.set_ghost(sections.iter().all(|(_, s)| s.all.borrow().is_empty()));
        for (src, s) in sections {
            let grid = self.clone();
            let busy = crate::Busy::new();
            glib::spawn_future_local(async move {
                let res = bg(source::titles(src.clone(), SOURCE_FETCH)).await;
                drop(busy);
                if grid.generation.get() != generation {
                    return;
                }
                grid.set_ghost(false);
                match res {
                    Ok(list) => {
                        source::save_titles(&src.url, &list);
                        s.all.replace(list);
                        grid.fill_sources();
                    }
                    Err(e) if s.all.borrow().is_empty() => {
                        eprintln!("Quelle {} nicht geladen: {e}", src.url);
                        s.title.set_label(&format!("{}  ›  (couldn't load: {e})", src.name));
                        s.title.set_visible(true);
                    }
                    Err(e) => eprintln!("Quelle {} nicht geladen, zeige gespeicherte Liste: {e}", src.url),
                }
            });
        }
    }

    /// Ergebnisse (Suche, ganze Quelle) im Bereich `results` statt der Startseite.
    fn show_results(self: &Rc<Self>, view: View, title: &str, fut: impl Future<Output = Result<Vec<Title>, String>> + Send + 'static) {
        let generation = self.begin(view);
        let w = self.column_width.get();
        for s in self.all_sections() {
            s.set(vec![], w);
        }
        for (_, s) in self.sources.borrow().iter() {
            s.title.set_visible(false);
        }
        self.results.title.set_label(title);
        self.set_ghost(true);
        let grid = self.clone();
        let busy = crate::Busy::new();
        glib::spawn_future_local(async move {
            let res = bg(fut).await;
            drop(busy);
            if grid.generation.get() != generation {
                return;
            }
            grid.set_ghost(false);
            match res {
                Ok(list) if list.is_empty() => grid.message("No results"),
                Ok(list) => grid.results.set(list.into_iter().map(Item::Title).collect(), grid.column_width.get()),
                Err(e) => grid.message(&format!("Error: {e}")),
            }
        });
    }

    pub fn show_search(self: &Rc<Self>, query: String) {
        self.show_results(View::Search(query.clone()), "", source::search(query));
    }

    pub fn show_source(self: &Rc<Self>, src: source::Source) {
        let name = src.name.clone();
        self.show_results(View::Source(src.clone()), &name, source::titles(src, SOURCE_ALL));
    }

    /// Zeigt das Raster die Startseite (nicht Suchergebnisse)?
    pub fn is_home(&self) -> bool {
        matches!(*self.view.borrow(), View::Home)
    }

    /// Aktuelle Seite neu laden.
    pub fn reload(self: &Rc<Self>) {
        let view = self.view.borrow().clone();
        match view {
            View::Home => self.show_home(),
            View::Search(q) => self.show_search(q),
            View::Source(s) => self.show_source(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_columns() {
        assert_eq!(fmt_duration(59), "0:59");
        assert_eq!(fmt_duration(3725), "1:02:05");
        // Spalten: schmal 1, Standardfenster (1280) 6, Vollbild 1920 → 9, 2560 → 13 → höchstens 12.
        assert_eq!(columns(150, None), 1);
        assert_eq!(columns(1248, None), 6);
        assert_eq!(columns(1888, None), 9);
        assert_eq!(columns(2528, None), 12);
        // Gewählt: so viele, wie 120-px-Karten passen.
        assert_eq!(columns(2528, Some(4)), 4);
        assert_eq!(columns(1248, Some(12)), 8);
        assert_eq!(step_columns(6, -1.0), 7);
        assert_eq!(step_columns(12, -1.0), 12);
        assert_eq!(step_columns(4, 1.0), 4);
        assert_eq!(poster_height(200), 300);
        // Quelle: 12 auf volle Reihen, Vorrat reicht nicht → nur volle Reihen.
        assert_eq!(shown_count(40, 6), 12);
        assert_eq!(shown_count(40, 5), 15);
        assert_eq!(shown_count(40, 12), 12);
        assert_eq!(shown_count(11, 6), 6);
        assert_eq!(shown_count(4, 6), 4);
    }
}
