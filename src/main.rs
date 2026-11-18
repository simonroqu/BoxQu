// Unter Windows ohne zusätzliches Konsolenfenster starten.
#![cfg_attr(windows, windows_subsystem = "windows")]

use boxqu_core::{aniskip, lists, progress, source};

mod grid;
mod player;
mod show;
mod watch;

use qu_core::{fly, search};
use source::Title;
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::{gdk, glib};
use progress::Resume;
use show::Play;
use std::cell::Cell;
use std::rc::Rc;

pub use qu_core::{bg, busy::Busy, RT};

fn build(app: &gtk::Application) -> Rc<dyn Fn(Option<String>)> {
    let display = gdk::Display::default().expect("Display");
    // Theme, Icons und CSS in Schichten: Kern < App (style.css) < ~/.config/<app>/style.css.
    qu_core::style::install(&display, include_str!("style.css"));

    let player = Rc::new(player::new());

    // Keine Kopfzeile: das Menü oben links baut qu-core aus `[nav]` in qu.toml.
    // Fenster, Seitenstapel und Menü oben links (aus qu.toml) baut qu-core.
    let shell = qu_core::nav::Shell::new(app);
    let navbar = shell.navbar.clone();
    let back = navbar.button("back").clone();
    let home = navbar.button("home").clone();
    let search_button = navbar.button("search").clone();

    let (stack, root, window) = (shell.stack.clone(), shell.root.clone(), shell.window.clone());

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

    // Seitenverlauf, Ansehen-Seite als Ebene darüber, Miniplayer (ziehbar, Zoom-Animation): qu-core.
    let nav = qu_core::nav::Nav::new(&shell, player.clone(), watch.frame());


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
            nav.window.set_title(Some(&format!("{name} – {}", qu_core::config().app.title)));
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
                    Ok(stream) => player.play(player::media(meta, stream)),
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
    let search = search::SearchModal::from_config(None, {
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

    // Vollbild-Knopf und Fenster-Vollbild regelt qu-core (`Nav::set_video_fullscreen`).
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
                gdk::Key::Escape if nav.video_fullscreen() => {
                    nav.set_video_fullscreen(false);
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

fn main() -> glib::ExitCode {
    qu_core::start(include_str!("../qu.toml"), include_str!("../data/dev.roqu.BoxQu.svg"), build)
}

#[cfg(test)]
mod tests {
    /// qu.toml ist gültig (Tippfehler, unbekannte Menüeinträge fallen hier auf statt beim Start).
    #[test]
    fn qu_toml() {
        let c = qu_core::config::Config::parse(include_str!("../qu.toml")).unwrap();
        assert_eq!(c.app.name, env!("CARGO_PKG_NAME"));
    }
}
