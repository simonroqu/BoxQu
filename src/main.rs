mod source;
mod moviebox;
mod aniskip;
mod controls;
mod fly;
mod grid;
mod icons;
mod lists;
mod nav_shape;
mod player;
mod progress;
mod search;
mod smooth;
mod show;
mod theme;
mod upscale;
mod watch;

use source::Title;
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use progress::Resume;
use show::Play;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::LazyLock;

const APP_NAME: &str = "BoxQu";

static RT: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| tokio::runtime::Runtime::new().expect("tokio"));

/// Führt `fut` im Hintergrund (tokio) aus; das Ergebnis kommt im GTK-Thread an.
pub async fn bg<T: Send + 'static>(fut: impl Future<Output = T> + Send + 'static) -> T {
    RT.spawn(fut).await.expect("Hintergrund-Task abgestürzt")
}

thread_local! {
    static BUSY_COUNT: Cell<u32> = const { Cell::new(0) };
    /// Lade-Kreis in der Nav-Bubble (neben der Suche).
    static BUSY_ICON: std::cell::OnceCell<gtk::Image> = const { std::cell::OnceCell::new() };
}

/// Solange ein `Busy` lebt, dreht sich der Lade-Kreis in der Nav-Bubble. Für jedes Laden
/// (Seiten, Episoden, Stream) eins anlegen und in den Ladevorgang mitnehmen.
pub struct Busy(());

impl Busy {
    pub fn new() -> Self {
        BUSY_COUNT.with(|c| c.set(c.get() + 1));
        busy_update();
        Busy(())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY_COUNT.with(|c| c.set(c.get().saturating_sub(1)));
        busy_update();
    }
}

fn busy_update() {
    let on = BUSY_COUNT.with(|c| c.get()) > 0;
    BUSY_ICON.with(|i| {
        if let Some(i) = i.get() {
            i.set_visible(on);
        }
    });
}

/// Seitenwechsel mit Verlauf für den Zurück-Button: "browse", "show", "watch".
struct Nav {
    stack: gtk::Stack,
    back: gtk::Button,
    window: gtk::ApplicationWindow,
    player: Rc<player::Player>,
    history: RefCell<Vec<String>>,
    /// Seiten, von denen man mit „Zurück“ gekommen ist (Maustaste 5 = vor). Neue Wege leeren sie.
    forward: RefCell<Vec<String>>,
    /// Schwebende Knöpfe oben links. Auf Übersicht/Titel-Seite immer sichtbar über der Seite;
    /// beim Video sitzen sie im Player und blenden sich mit dessen Leiste aus und ein.
    nav_bar: gtk::Revealer,
    page_layer: gtk::Overlay,
    in_player: Cell<bool>,
    watch: Rc<watch::WatchPage>,
    /// Umgibt die Ansehen-Seite für die Öffnen-/Miniplayer-Animation (fly.rs).
    zoom: fly::Zoom,
    /// Knöpfe im Miniplayer (Pause, Ton, Zurück zum Video, Schließen).
    mini_bar: gtk::Box,
}

impl Nav {
    fn current(&self) -> String {
        self.stack.visible_child_name().map(|s| s.to_string()).unwrap_or_default()
    }

    fn show(&self, page: &str) {
        let watch = page == "watch";
        // Ansehen-Seite erscheint (neu oder aus dem Miniplayer): kurz einblenden.
        let appear = watch && (self.current() != "watch" || self.watch.is_mini());
        // Verlässt man die Ansehen-Seite, läuft ein laufendes Video im Miniplayer weiter, sonst stoppt es.
        if self.current() == "watch" && !watch {
            self.window.unfullscreen();
            if self.player.active() {
                self.set_mini(true);
            } else {
                self.close_mini();
            }
        }
        if watch {
            self.set_mini(false);
        } else if self.watch.is_mini() && !self.player.active() {
            // Im Miniplayer zu Ende gelaufen (z. B. „Home“ im Endbildschirm): weg damit.
            self.close_mini();
        }
        self.watch.widget.set_visible(watch || self.watch.is_mini());
        if appear && !self.zoom.animating() {
            // Die CSS-Animation startet nur neu, wenn sich ihr Name ändert: zwischen a und b wechseln.
            let w = &self.watch.widget;
            let a = w.has_css_class("page-in-a");
            w.remove_css_class(if a { "page-in-a" } else { "page-in-b" });
            w.add_css_class(if a { "page-in-b" } else { "page-in-a" });
        }
        // Von/zur Ansehen-Seite ohne Überblendung: die liegt als eigene Ebene darüber (Zoom-Animation),
        // im Stack steht nur ein leerer Platzhalter. Mit Überblendung wäre es beim Schrumpfen dahinter schwarz.
        let fade = !watch && self.current() != "watch";
        self.stack.set_transition_type(if fade { gtk::StackTransitionType::Crossfade } else { gtk::StackTransitionType::None });
        self.stack.set_visible_child_name(page);
        self.back.set_visible(!self.history.borrow().is_empty());

        if watch != self.in_player.get() {
            let (from, to) = if watch { (&self.page_layer, &self.player.widget) } else { (&self.player.widget, &self.page_layer) };
            from.remove_overlay(&self.nav_bar);
            to.add_overlay(&self.nav_bar);
            self.in_player.set(watch);
        }
        self.nav_bar.set_reveal_child(!watch || self.player.controls.widget.reveals_child());
    }

    fn set_mini(&self, on: bool) {
        // Wechsel groß ↔ klein bei laufendem Video: die Seite schrumpft/wächst vom alten Platz aus.
        let from = (self.watch.is_mini() != on && self.player.active()).then(|| self.zoom.anchor_bounds()).flatten();
        // Beim Großwerden bleibt die Seite darunter stehen, bis die Animation fertig ist.
        let backdrop = from.filter(|_| !on).and_then(|_| self.zoom.backdrop(&self.stack));
        self.watch.set_mini(on);
        if let Some(from) = from {
            self.zoom.animate_from(from, backdrop);
        }
        // Mini-Knöpfe erst nach dem Schrumpfen: während der Animation wird die schon kleine Seite groß
        // gezeichnet, die Knöpfe wären riesig. Danach nur, wenn der Player dann noch klein ist.
        self.mini_bar.set_visible(false);
        if on {
            let (bar, watch) = (self.mini_bar.clone(), self.watch.clone());
            self.zoom.after(move || bar.set_visible(watch.is_mini()));
        }
        // Die große Leiste passt nicht in den kleinen Player.
        self.player.controls.widget.set_visible(!on);
    }

    /// Miniplayer schließen bzw. Video beenden (speichert die Position).
    fn close_mini(&self) {
        // Das Video hat den Tastatur-Fokus. Verschwindet es damit, gibt GTK den Fokus der ersten Karte,
        // und die Liste springt nach oben. Darum vorher den Fokus abgeben.
        gtk::prelude::GtkWindowExt::set_focus(&self.window, None::<&gtk::Widget>);
        self.player.stop();
        self.set_mini(false);
        self.watch.widget.set_visible(self.current() == "watch");
        self.window.set_title(Some(APP_NAME));
    }

    fn go(&self, page: &str) {
        let cur = self.current();
        if cur != page {
            self.history.borrow_mut().push(cur);
            self.forward.borrow_mut().clear();
        }
        self.show(page);
    }

    fn back(&self) {
        let prev = self.history.borrow_mut().pop();
        if let Some(p) = prev {
            self.forward.borrow_mut().push(self.current());
            self.show(&p);
        }
    }

    /// Gegenstück zu `back`. ponytail: merkt nur die Seite, nicht ihren Inhalt – die Seiten gibt es je
    /// einmal und sie zeigen noch, was zuletzt drin war (Titel, Episode).
    fn forward(&self) {
        let next = self.forward.borrow_mut().pop();
        if let Some(p) = next {
            self.history.borrow_mut().push(self.current());
            self.show(&p);
        }
    }

    fn home(&self) {
        self.history.borrow_mut().clear();
        self.forward.borrow_mut().clear();
        self.show("browse");
    }
}

/// Läuft die App unter einem Tiling-Window-Manager?
/// ponytail: kurze Liste bekannter Tiling-WMs; erweitern, wenn jemand einen anderen nutzt.
fn tiling_wm() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    ["hyprland", "sway", "niri", "river", "i3"].iter().any(|wm| desktop.contains(wm))
}

fn build(app: &gtk::Application) -> Rc<dyn Fn(Option<String>)> {
    let display = gdk::Display::default().expect("Display");
    let css = gtk::CssProvider::new();
    css.load_from_string(include_str!("style.css"));
    // Eine Stufe über dem Theme (theme.rs): gezielte Regeln wie die Player-Bubbles sollen gewinnen,
    // GTK entscheidet bei gleicher Priorität nach Ladereihenfolge, nicht nach Genauigkeit.
    gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    theme::install(&display);
    icons::install(&display);

    let player = Rc::new(player::Player::new());

    // Keine Kopfzeile: Zurück, Home und Suche schweben oben links in einer Bubble.
    let nav_button = |icon: &str, tip: &str| {
        gtk::Button::builder().icon_name(icon).tooltip_text(tip).focusable(false).css_classes(["flat"]).build()
    };
    let back = nav_button("boxqu-back-symbolic", "Back (Esc)");
    back.set_visible(false);
    let home = nav_button("boxqu-home-symbolic", "Home");
    let search_button = nav_button("boxqu-search-symbolic", "Search (/)");
    let top = gtk::Box::builder().spacing(2).height_request(32).build();
    top.append(&back);
    top.append(&home);
    top.append(&search_button);
    let busy_icon = icons::spinner(16).visible(false).margin_start(6).margin_end(6).build();
    BUSY_ICON.with(|i| i.set(busy_icon.clone()).ok());
    top.append(&busy_icon);
    let nav_box = nav_shape::NavShape::new();
    nav_box.set_css_classes(&["bubble", "nav-shape"]);
    nav_box.append(&top);
    let nav_bar = gtk::Revealer::builder()
        .child(&nav_box)
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .reveal_child(true)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .css_classes(["nav-bubble"])
        .build();

    // Nicht homogen: sonst bestimmt die breiteste (auch unsichtbare) Seite die Mindestbreite aller Seiten.
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .hhomogeneous(false)
        .vhomogeneous(false)
        .build();
    // Ebene 1: Seiten plus Navigation. Ebene 2 (darüber): Suchfenster und Fenster-Knöpfe.
    let page_layer = gtk::Overlay::builder().child(&stack).build();
    page_layer.add_overlay(&nav_bar);
    let root = gtk::Overlay::builder().child(&page_layer).build();
    // Unter Tiling-Window-Managern schließt man Fenster per Tastenkürzel: keine Fenster-Knöpfe.
    // Sonst (GNOME, KDE, Windows …) oben rechts in einer eigenen Bubble.
    if !tiling_wm() {
        let controls = gtk::WindowControls::builder().side(gtk::PackType::End).css_classes(["bubble"]).build();
        let wc = gtk::Box::builder().halign(gtk::Align::End).valign(gtk::Align::Start).css_classes(["nav-bubble"]).build();
        wc.append(&controls);
        root.add_overlay(&wc);
    }

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(APP_NAME)
        .default_width(1280)
        .default_height(860)
        .decorated(false)
        .build();
    if tiling_wm() {
        window.set_child(Some(&root));
    } else {
        // Ohne Titelleiste: an freien Flächen ziehen verschiebt das Fenster.
        window.set_child(Some(&gtk::WindowHandle::builder().child(&root).build()));
    }

    // Titel-Seite öffnen. Wird erst unten befüllt, weil Titel-Seite und Ansehen-Seite sich gegenseitig brauchen.
    let open_show_cell: Rc<std::cell::OnceCell<Rc<dyn Fn(Title)>>> = Rc::default();
    let open_show_late: Rc<dyn Fn(Title)> = Rc::new({
        let cell = open_show_cell.clone();
        move |a| {
            if let Some(f) = cell.get() {
                f(a)
            }
        }
    });

    // Ansehen-Seite: Video oben, darunter Titel-Titel (öffnet die Titel-Seite) und Episode.
    let watch_cell: Rc<std::cell::OnceCell<Rc<watch::WatchPage>>> = Rc::default();
    let watch = watch::WatchPage::new(&player.widget, {
        let (open_show, watch_cell) = (open_show_late.clone(), watch_cell.clone());
        move || {
            let current = watch_cell.get().and_then(|w| w.current.borrow().clone());
            if let Some(p) = current {
                open_show(p.anime);
            }
        }
    });
    let _ = watch_cell.set(watch.clone());

    // Miniplayer-Knöpfe in den Ecken, je eine Bubble: oben links groß machen, oben rechts schließen,
    // unten links Play und Ton. Die Symbole folgen den Knöpfen der großen Leiste.
    let mini_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(8)
        .visible(false)
        .build();
    let bubble = || gtk::Box::builder().spacing(2).height_request(32).css_classes(["bubble"]).build();
    let (mini_top_left, mini_top_right, mini_bottom) = (bubble(), bubble(), bubble());
    let mini_play = nav_button("boxqu-pause-symbolic", "Play/Pause");
    player.controls.play.bind_property("icon-name", &mini_play, "icon-name").sync_create().build();
    let mini_mute = nav_button("boxqu-volume-symbolic", "Mute");
    player.controls.mute.bind_property("icon-name", &mini_mute, "icon-name").sync_create().build();
    let mini_open = nav_button("boxqu-fullscreen-symbolic", "Back to video");
    let mini_close = nav_button("boxqu-close-symbolic", "Close");
    mini_top_left.append(&mini_open);
    mini_top_right.append(&mini_close);
    mini_bottom.append(&mini_play);
    mini_bottom.append(&mini_mute);
    let top = gtk::CenterBox::builder().start_widget(&mini_top_left).end_widget(&mini_top_right).valign(gtk::Align::Start).vexpand(true).build();
    mini_bottom.set_halign(gtk::Align::Start);
    mini_bottom.set_valign(gtk::Align::End);
    mini_bar.append(&top);
    mini_bar.append(&mini_bottom);
    mini_play.connect_clicked({
        let mpv = player.mpv;
        move |_| {
            let _ = mpv.command("cycle", &["pause"]);
        }
    });
    mini_mute.connect_clicked({
        let controls = player.controls.clone();
        move |_| controls.toggle_mute()
    });
    watch.widget.add_overlay(&mini_bar);
    // Über den Seiten, unter Navigation (wird beim Seitenwechsel neu obenauf gelegt) und Suche.
    // In `Zoom` verpackt: geht beim Öffnen aus Karte/Kachel auf, Miniplayer ebenso (fly.rs).
    let zoom = fly::Zoom::new(&watch.widget, &player.widget);
    page_layer.add_overlay(&zoom);
    // Miniplayer ziehen; beim Loslassen gleitet er in die nächste Ecke.
    let drag = gtk::GestureDrag::new();
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let dragging = Rc::new(Cell::new(false));
    drag.connect_drag_update({
        let (watch, zoom, dragging) = (watch.clone(), zoom.clone(), dragging.clone());
        move |g, dx, dy| {
            if !watch.is_mini() {
                return;
            }
            // Erst ab ein paar Pixeln ist es Ziehen; sonst bleibt es ein Klick (Knöpfe im Miniplayer).
            if !dragging.get() && dx.hypot(dy) < 8.0 {
                return;
            }
            if !dragging.get() {
                dragging.set(true);
                g.set_state(gtk::EventSequenceState::Claimed);
            }
            zoom.set_offset(dx as f32, dy as f32);
        }
    });
    // Loslassen nach Ziehen um (dx, dy): in die nächste Ecke gleiten.
    let snap: Rc<dyn Fn(f32, f32)> = Rc::new({
        let (watch, zoom) = (watch.clone(), zoom.clone());
        move |dx, dy| {
            zoom.set_offset(0.0, 0.0);
            let (Some(player), Some(page)) = (zoom.anchor_bounds(), watch.widget.compute_bounds(&zoom)) else { return };
            // Nächste Ecke = Seite, auf der die Mitte des Miniplayers jetzt liegt.
            let right = page.x() + dx + page.width() / 2.0 > zoom.width() as f32 / 2.0;
            let bottom = page.y() + dy + page.height() / 2.0 > zoom.height() as f32 / 2.0;
            watch.set_corner(right, bottom);
            let from = gtk::graphene::Rect::new(player.x() + dx, player.y() + dy, player.width(), player.height());
            zoom.animate_from(from, None);
        }
    });
    drag.connect_drag_end({
        let snap = snap.clone();
        move |_, dx, dy| {
            if dragging.replace(false) {
                snap(dx as f32, dy as f32);
            }
        }
    });
    watch.widget.add_controller(drag);

    let nav = Rc::new(Nav {
        stack: stack.clone(),
        back: back.clone(),
        window: window.clone(),
        player: player.clone(),
        history: RefCell::default(),
        forward: RefCell::default(),
        nav_bar: nav_bar.clone(),
        page_layer: page_layer.clone(),
        in_player: Cell::new(false),
        watch: watch.clone(),
        zoom: zoom.clone(),
        mini_bar: mini_bar.clone(),
    });
    mini_open.connect_clicked({
        let nav = nav.clone();
        move |_| nav.go("watch")
    });
    mini_close.connect_clicked({
        let nav = nav.clone();
        move |_| nav.close_mini()
    });
    // Beim Video: Navigation blendet sich zusammen mit der Player-Leiste ein und aus.
    player.controls.widget.connect_reveal_child_notify({
        let nav = nav.clone();
        move |r| {
            if nav.in_player.get() {
                nav.nav_bar.set_reveal_child(r.reveals_child());
            }
        }
    });


    // Episode abspielen: Seite sofort, Stream-Adresse im Hintergrund (Archive: eine Anfrage).
    let play_gen: Rc<Cell<u64>> = Rc::default();
    let play: Rc<dyn Fn(Play)> = Rc::new({
        let (nav, player, watch) = (nav.clone(), player.clone(), watch.clone());
        move |p: Play| {
            let generation = play_gen.get() + 1;
            play_gen.set(generation);
            // Seite geht aus der angeklickten Episoden-Kachel bzw. „Continue watching“-Karte auf
            // (vor dem Seitenwechsel messen). Nicht, wenn die Ansehen-Seite schon groß offen ist (nächste Folge).
            let from = (nav.current() != "watch" || nav.watch.is_mini())
                .then(|| fly::thumb_bounds(&p.episode().id, &nav.zoom).or_else(|| fly::thumb_bounds(&p.anime.id, &nav.zoom)))
                .flatten();
            let backdrop = from.and_then(|_| nav.zoom.backdrop(&nav.stack));
            watch.show(&p);
            nav.go("watch");
            if let Some(from) = from {
                // Nach `go`: übernimmt auch einen Miniplayer-Übergang; die Einblendung von `show` braucht es dann nicht.
                nav.zoom.animate_from(from, backdrop);
                nav.watch.widget.remove_css_class("page-in-a");
                nav.watch.widget.remove_css_class("page-in-b");
            }
            player.loading();
            player.end_next.set_visible(p.next().is_some());
            let ep = p.episode().clone();
            // Fenstertitel = Titel und Episode (Alt-Tab, Fensterliste).
            let name = if p.episodes.len() > 1 { format!("{} · Episode {}", p.anime.title, ep.number) } else { p.anime.title.clone() };
            nav.window.set_title(Some(&format!("{name} – {APP_NAME}")));
            let meta = progress::Meta {
                anime: p.anime.id.clone(),
                episode: ep.number,
                title: p.anime.title.clone(),
                poster: p.anime.poster.clone(),
                total: p.episodes.len() as u32,
            };
            let (player, play_gen) = (player.clone(), play_gen.clone());
            let busy = Busy::new();
            glib::spawn_future_local(async move {
                let res = bg(source::stream(ep.id)).await;
                drop(busy);
                // Inzwischen eine andere Episode gewählt: verwerfen.
                if play_gen.get() != generation {
                    return;
                }
                match res {
                    Ok(stream) => player.play(meta, stream),
                    Err(e) => player.show_error(&format!("Couldn't load episode:\n{e}")),
                }
            });
        }
    });
    // Aktuelle Episode neu laden (Reload, Reload-Icon unter Fehlern): holt eine frische Stream-Adresse.
    let replay: Rc<dyn Fn()> = Rc::new({
        let (watch, play) = (watch.clone(), play.clone());
        move || {
            let current = watch.current.borrow().clone();
            if let Some(p) = current {
                play(p);
            }
        }
    });
    let play_next: Rc<dyn Fn()> = Rc::new({
        let (watch, play) = (watch.clone(), play.clone());
        move || {
            let next = watch.current.borrow().as_ref().and_then(|p| p.next());
            if let Some(p) = next {
                play(p);
            }
        }
    });
    watch.next.connect_clicked({
        let play_next = play_next.clone();
        move |_| play_next()
    });
    player.end_next.connect_clicked({
        let play_next = play_next.clone();
        move |_| play_next()
    });
    watch.prev.connect_clicked({
        let (watch, play) = (watch.clone(), play.clone());
        move |_| {
            let prev = watch.current.borrow().as_ref().and_then(|p| p.prev());
            if let Some(p) = prev {
                play(p);
            }
        }
    });

    let show_page = show::ShowPage::new({
        let play = play.clone();
        move |p| play(p)
    });
    let open_show: Rc<dyn Fn(Title)> = Rc::new({
        let (nav, show_page) = (nav.clone(), show_page.clone());
        move |a| {
            show_page.open(a);
            nav.go("show");
        }
    });
    let _ = open_show_cell.set(open_show.clone());

    // Episode eines Titel abspielen, von dem nur die ID bekannt ist („Continue watching“, Links):
    // Episodenliste holen, dann abspielen. Klappt das nicht, die Titel-Seite öffnen (zeigt dort den Fehler).
    let play_episode: Rc<dyn Fn(Title, Box<dyn Fn(&source::Episode) -> bool>)> = Rc::new({
        let (play, open_show) = (play.clone(), open_show.clone());
        move |a, wanted| {
            let (play, open_show) = (play.clone(), open_show.clone());
            let busy = Busy::new();
            glib::spawn_future_local(async move {
                // Nur die ID bekannt (Link): Titel und Poster gleichzeitig von der Titel-Seite holen.
                let (id, need_details) = (a.id.clone(), a.title.is_empty());
                let (res, details) = bg(async move {
                    let d = need_details.then(|| tokio::spawn(source::details(id.clone())));
                    let eps = source::episodes(id).await;
                    let details = match d {
                        Some(d) => d.await.ok().and_then(Result::ok),
                        None => None,
                    };
                    (eps, details)
                })
                .await;
                drop(busy);
                let a = details.map_or(a, |d| d.item);
                let index = res.as_ref().ok().and_then(|eps| eps.iter().position(|e| wanted(e)));
                match (res, index) {
                    (Ok(eps), Some(index)) => {
                        play(Play { anime: a, episodes: Rc::new(eps), index });
                    }
                    _ => open_show(a),
                }
            });
        }
    });
    let resume: Rc<dyn Fn(Resume)> = Rc::new({
        let play_episode = play_episode.clone();
        move |r: Resume| play_episode(r.anime, Box::new(move |e| e.number == r.episode))
    });
    let grid = grid::Grid::new(
        {
            let open_show = open_show.clone();
            move |a| open_show(a)
        },
        {
            let resume = resume.clone();
            move |r| resume(r)
        },
    );
    // Eingefügter Link (Strg+V, Suche, Startargument): Quelle speichern, Titel öffnen oder Video abspielen.
    let open_link: Rc<dyn Fn(&str)> = Rc::new({
        let (nav, grid, play_episode, open_show) = (nav.clone(), grid.clone(), play_episode.clone(), open_show.clone());
        move |url| {
            let (nav, grid, play_episode, open_show) = (nav.clone(), grid.clone(), play_episode.clone(), open_show.clone());
            let url = url.to_string();
            let busy = Busy::new();
            glib::spawn_future_local(async move {
                let res = bg(source::open_link(url)).await;
                drop(busy);
                match res {
                    Ok(source::Link::Source) => {
                        nav.home();
                        grid.show_home();
                    }
                    Ok(source::Link::Title(t)) => open_show(t),
                    Ok(source::Link::Video(t)) => play_episode(t, Box::new(|_| true)),
                    Err(e) => {
                        nav.home();
                        grid.message(&format!("Couldn't open link: {e}"));
                    }
                }
            });
        }
    });
    stack.add_named(&grid.widget, Some("browse"));
    stack.add_named(&show_page.widget, Some("show"));
    // Platzhalter: die Ansehen-Seite selbst liegt als Ebene darüber (damit sie auch als Miniplayer geht).
    stack.add_named(&gtk::Box::new(gtk::Orientation::Horizontal, 0), Some("watch"));

    // Reload: aktuelle Seite neu laden (Startseite/Suche, Titel-Seite oder Episode).
    let reload_page: Rc<dyn Fn()> = Rc::new({
        let (nav, grid, show_page, replay) = (nav.clone(), grid.clone(), show_page.clone(), replay.clone());
        move || match nav.current().as_str() {
            "browse" => grid.reload(),
            "show" => show_page.reload(),
            "watch" => replay(),
            _ => {}
        }
    });
    player.retry.connect_clicked({
        let replay = replay.clone();
        move |_| replay()
    });

    back.connect_clicked({
        let nav = nav.clone();
        move |_| nav.back()
    });
    // Seitentasten der Maus (GTK: 8 = zurück, 9 = vor), überall im Fenster, auch über dem Player.
    let side_buttons = gtk::GestureClick::builder().button(0).propagation_phase(gtk::PropagationPhase::Capture).build();
    side_buttons.connect_pressed({
        let nav = nav.clone();
        move |g, _, _, _| match g.current_button() {
            8 => {
                g.set_state(gtk::EventSequenceState::Claimed);
                nav.back();
            }
            9 => {
                g.set_state(gtk::EventSequenceState::Claimed);
                nav.forward();
            }
            _ => {}
        }
    });
    window.add_controller(side_buttons);
    // Home: Startseite (statt Suchergebnissen); ist man schon dort, neu laden.
    home.connect_clicked({
        let (nav, grid) = (nav.clone(), grid.clone());
        move |_| {
            let was_home = nav.current() == "browse" && grid.is_home();
            nav.home();
            if was_home {
                grid.reload();
            } else if !grid.is_home() {
                grid.show_home();
            }
        }
    });

    // Schwebendes Suchfenster über allem.
    let search = search::SearchModal::new({
        let (grid, nav, open_link) = (grid.clone(), nav.clone(), open_link.clone());
        move |q| {
            if q.starts_with("http") {
                return open_link(&q);
            }
            nav.home();
            grid.show_search(q);
        }
    });
    root.add_overlay(&search.widget);
    search_button.connect_clicked({
        let search = search.clone();
        move |_| search.open()
    });
    // Home (auch der Endbildschirm-Knopf) verlässt die Suche: Feld beim nächsten Öffnen leer.
    home.connect_clicked({
        let search = search.clone();
        move |_| search.reset()
    });

    // Vollbild: Infos ausblenden, nur noch Video.
    player.controls.fullscreen.connect_clicked({
        let window = window.clone();
        move |_| {
            if window.is_fullscreen() { window.unfullscreen() } else { window.fullscreen() }
        }
    });
    window.connect_fullscreened_notify({
        let (watch, fs) = (watch.clone(), player.controls.fullscreen.clone());
        move |w| {
            let full = w.is_fullscreen();
            watch.set_fullscreen(full);
            fs.set_icon_name(if full { "boxqu-restore-symbolic" } else { "boxqu-fullscreen-symbolic" });
        }
    });
    // Endbildschirm „Zur Startseite“.
    player.end_home.connect_clicked({
        let home = home.clone();
        move |_| home.emit_clicked()
    });
    // Während ein Video läuft: kein Bildschirmschoner / keine Sperre (über GTK, also systemunabhängig).
    player.connect_playing({
        let (app, window) = (app.clone(), window.clone());
        let cookie = Cell::new(0u32);
        move |playing| {
            if playing && cookie.get() == 0 {
                cookie.set(app.inhibit(Some(&window), gtk::ApplicationInhibitFlags::IDLE, Some("Playing a video")));
            } else if !playing && cookie.get() != 0 {
                app.uninhibit(cookie.replace(0));
            }
        }
    });
    // Player-Höhe an das Seitenverhältnis des Videos anpassen.
    player.connect_aspect({
        let watch = watch.clone();
        move |a| watch.set_aspect(a)
    });

    // Tastenkürzel. Tippt man im Suchfeld, kommen die Tasten hier nicht an.
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let (nav, window, player, search, open_link) = (nav.clone(), window.clone(), player.clone(), search.clone(), open_link.clone());
        let (reload_page, play_next) = (reload_page.clone(), play_next.clone());
        move |_, key, _, mods| {
            // Suchfenster offen: nur Esc schließt, alles andere gehört der Suche.
            if search.is_open() {
                if key == gdk::Key::Escape {
                    search.close();
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            }
            // Strg+V: Link aus der Zwischenablage (Quelle, Archive-Eintrag, Video).
            if key == gdk::Key::v && mods.contains(gdk::ModifierType::CONTROL_MASK) {
                let open_link = open_link.clone();
                let clipboard = window.clipboard();
                glib::spawn_future_local(async move {
                    if let Ok(Some(text)) = clipboard.read_text_future().await {
                        open_link(text.trim());
                    }
                });
                return glib::Propagation::Stop;
            }
            // Suche öffnen: "/" oder Strg+K.
            if key == gdk::Key::slash || (key == gdk::Key::k && mods.contains(gdk::ModifierType::CONTROL_MASK)) {
                search.open();
                return glib::Propagation::Stop;
            }
            // Neu laden: F5, Strg+R oder R.
            if key == gdk::Key::F5 || (key == gdk::Key::r && mods.contains(gdk::ModifierType::CONTROL_MASK)) {
                reload_page();
                return glib::Propagation::Stop;
            }
            // Shift ist erlaubt (">" braucht es auf deutschen Tastaturen), andere Modifier nicht.
            if !(mods - gdk::ModifierType::SHIFT_MASK).is_empty() {
                return glib::Propagation::Proceed;
            }
            let watching = nav.current() == "watch";
            // Miniplayer: nur die Video-Tasten; Pfeile/Ziffern bleiben fürs Raster.
            let mini = !watching && nav.watch.is_mini();
            let handled = match key {
                gdk::Key::Escape if window.is_fullscreen() => {
                    window.unfullscreen();
                    true
                }
                gdk::Key::Escape => {
                    nav.back();
                    true
                }
                gdk::Key::r | gdk::Key::R => {
                    reload_page();
                    true
                }
                gdk::Key::f if watching => {
                    player.controls.fullscreen.emit_clicked();
                    true
                }
                // f: zurück zum großen Video (Ansehen-Seite, kein Vollbild); q: Miniplayer schließen (wie in mpv).
                gdk::Key::f if mini => {
                    nav.go("watch");
                    true
                }
                gdk::Key::q if mini => {
                    nav.close_mini();
                    true
                }
                gdk::Key::space | gdk::Key::k | gdk::Key::m if mini => player.key(key),
                gdk::Key::n | gdk::Key::N if watching => {
                    play_next();
                    true
                }
                k if watching => player.key(k),
                // Pfeiltasten, solange nichts auf der Seite gewählt ist (nach dem Start, mit Miniplayer):
                // erste Karte wählen, danach navigiert GTK selbst im Raster weiter (Enter öffnet).
                gdk::Key::Up | gdk::Key::Down | gdk::Key::Left | gdk::Key::Right => {
                    let page = nav.stack.visible_child();
                    match page {
                        Some(p) if gtk::prelude::GtkWindowExt::focus(&window).is_none_or(|f| !f.is_ancestor(&p)) => p.child_focus(gtk::DirectionType::TabForward),
                        _ => false,
                    }
                }
                _ => false,
            };
            if handled { glib::Propagation::Stop } else { glib::Propagation::Proceed }
        }
    });
    window.add_controller(keys);

    // Beim Schließen die Position der laufenden Episode sichern.
    window.connect_close_request({
        let player = player.clone();
        move |_| {
            player.stop();
            glib::Propagation::Proceed
        }
    });

    // Öffnet, was beim Start übergeben wurde, auch wenn die App schon läuft:
    // `boxqu <link>` wie Strg+V, `boxqu <suchbegriff>` sucht,
    // `boxqu` allein zeigt beim ersten Mal die Startseite und holt sonst nur das Fenster nach vorne.
    let started = Cell::new(false);
    Rc::new(move |arg: Option<String>| {
        match arg {
            Some(arg) if arg.starts_with("http") => open_link(&arg),
            Some(arg) => {
                nav.home();
                grid.show_search(arg);
            }
            None if !started.get() => grid.show_home(),
            None => {}
        }
        started.set(true);
        window.present();
        player.prepare();
    })
}

thread_local! {
    /// Das Fenster gibt es nur einmal; weitere Starts geben ihre Argumente hierher weiter.
    static OPEN: std::cell::OnceCell<Rc<dyn Fn(Option<String>)>> = const { std::cell::OnceCell::new() };
}

fn main() -> glib::ExitCode {
    // libmpv verlangt LC_NUMERIC=C; ohne setlocale durch GTK bleibt alles auf C.
    gtk::disable_setlocale();
    let app = gtk::Application::builder()
        .application_id("dev.roqu.BoxQu")
        .flags(if std::env::var_os("BOXQU_DEV").is_some() {
            // Entwicklung: eigene Instanz neben der installierten, statt Aufrufe an sie weiterzugeben.
            gio::ApplicationFlags::HANDLES_COMMAND_LINE | gio::ApplicationFlags::NON_UNIQUE
        } else {
            gio::ApplicationFlags::HANDLES_COMMAND_LINE
        })
        .build();
    // Läuft die App schon, landet ein weiterer Start hier im laufenden Prozess.
    app.connect_command_line(|app, cmdline| {
        let arg = cmdline.arguments().get(1).map(|a| a.to_string_lossy().into_owned());
        let open = OPEN.with(|o| o.get_or_init(|| build(app)).clone());
        open(arg);
        glib::ExitCode::SUCCESS
    });
    app.run()
}

