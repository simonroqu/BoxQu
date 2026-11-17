# BoxQu – Film- und Serien-Player für Linux (Rust + GTK4 + libmpv)

Film- und Serien-Player für MovieBox im Stil von AniQu (`~/Work/aniqu`, über MovQu `~/Work/movqu` übernommen): gleicher
Aufbau und Style; gemeinsamer Unterbau über **qu-core** (`~/Work/quCore`). App-ID `dev.roqu.BoxQu`. **Eigenständig**: spricht selbst mit der MovieBox-API
(`moviebox.rs` + `source.rs`, portiert aus MovieBox-Tui, MIT/Apache-2.0). MovieBox-Tui (`~/Work/MovieBox-Tui`, Klon von
upstream, dort nichts ändern) ist nur noch Nachschlagewerk, wenn MovieBox etwas an der API ändert.

**Code-Kommentare: Deutsch. Alle Texte in der Oberfläche: Englisch.**

**Diese Datei bei jeder Änderung am Projekt mitpflegen** (gleicher Schritt wie die Code-Änderung), damit eine
neue Session sofort Bescheid weiß. Nur Entwicklungsrelevantes, keine persönlichen Bemerkungen.

## Bauen, starten, installieren

```bash
cargo build && cargo test            # Unit-Tests, ohne Netzwerk
cargo test -- --ignored              # Live-Test gegen MovieBox (wenige Anfragen)
BOXQU_DEV=1 cargo run                # eigene Instanz neben der installierten
cargo run -- dune                    # direkt suchen
./install.sh                         # ~/.local/bin/boxqu + Startmenü-Eintrag + Icon
```

Eine laufende installierte Instanz bekommt das neue Binary erst nach einem Neustart.

## Aufbau (`src/`)

| Datei | Aufgabe |
|---|---|
| `main.rs` | Fenster, Navigation (`Nav`: `browse`/`show`/`watch`, Verlauf zurück/vor), Nav-Bubble, Tasten, Maus-Seitentasten, Startargumente, `play`/`resume`, `open_link` (Strg+V, Link in der Suche, Startargument) |
| `moviebox.rs` | MovieBox-API: Gerät (User-Agent/`x-client-info`, einmal pro Start ausgewürfelt), Signatur (`x-tr-signature` HMAC-MD5, `x-client-build`), Besucher-Login (Token in `~/.cache/boxqu/moviebox-session`, `exp` aus dem JWT), Host-Pool mit Weiterschalten; liefert rohes JSON (`data`) |
| `source.rs` | **Einzige Stelle, die Inhalte holt**: Endpunkte + JSON → Typen. Feste Startseiten-Bereiche, Suche, Titel-Seite, Episoden, Stream + Untertitel, Poster, Listen-Cache |
| `aniskip.rs` | aus AniQu, derzeit ungenutzt (`Stream::mal_id` immer `None`); `Segment` treibt den „Skip intro“-Knopf – Platz für spätere Intro-Zeiten (z. B. Jellyfin Media Segments) |
| `grid.rs` | Poster-Raster: Startseite („Continue watching“, Watchlist, dann je Quelle ein Bereich mit ~12 Titeln auf volle Reihen, ohne Watchlist-Titel), Überschrift einer Quelle: Klick = alle Titel (`show_source`), Rechtsklick = entfernen; Rechtsklick-Menü der Karten (`card_menu`), Suche; Strg+Mausrad = Spalten 4–12 (`~/.config/boxqu/columns`) |
| `show.rs` | Titel-Seite: Poster, Infos, Beschreibung, Sprach-Tabs (`set_languages`), Lesezeichen-Knopf (Watchlist), Episoden-Kacheln (Film = eine Kachel „Play“); `Play` (Episode + Liste). `open` springt zur bevorzugten Sprachfassung, `open_as(…, false)` nicht (Tab-Klick, Reload) |
| `player.rs` | Player-Anbindung an qu-core: `Hooks` (User-Agent, Fortschritt pro Episode, Tempo pro Titel) und `media(meta, stream)` (HTTP-Header, Untertitel-Dateien, Fortsetzen). Item = `(Meta, MAL-ID)` |
| `watch.rs` | Infos unter dem Video (Titel → Titel-Seite, Episode, vorige/nächste) im `WatchFrame` aus qu-core |
| `progress.rs` | Fortschritt pro Episode, „Continue watching“ (fertig → nächste Episode; `total` 1 = Film, Karte ohne Episodennummer); „Watched“ = `hide` (`hidden.tsv`: ID + Zeitpunkt): Titel bleibt aus „Continue watching“, bis danach wieder eine Episode gespeichert wird (neuerer Zeitstempel) |
| `lists.rs` | Suchverlauf, Watchlist (`~/.local/share/boxqu/watchlist.tsv`: `source::read_list`/`write_list`), Tempo pro Titel, bevorzugte Sprache (`language`, `preferred`) |
| `qu.toml` | App-Konfiguration für qu-core: Name/Titel/ID/Startmenü-Texte, **Menü oben links** (`[nav]`), Suche, Player, Fortschrittsregeln, Raster. Format in `~/Work/quCore/CLAUDE.md`; Test `qu_toml` in `main.rs` prüft sie |
| **qu-core** (`~/Work/quCore`) | Gemeinsamer Unterbau, **dort ändern**: App-Start (`start`, `--install`), Dateien/Tempo (`store`), Fortschrittsregeln (`progress`), Raster-Bausteine (`cards`), Fenster/Seitenverlauf/Miniplayer (`nav`), Player (`player`, `controls`, Rahmen `watch`), Menü (`navbar`), Ladeanzeige (`Busy`), `bg`, CSS-Schichten (`style`), `app` (Pfade), `theme`, gemeinsames CSS (`qu_core::CSS`), `icons` (`qu-*-symbolic`), `nav_shape`, `fly`, `smooth`, `search` (Suchfenster + Suchverlauf), `upscale` (Shader-Set hier: `UPSCALER` in `controls.rs`). Beschreibung in `~/Work/quCore/CLAUDE.md` |
| `style.css` | Nur App-eigenes CSS (Poster-Badges `.badge`, Episoden-Kacheln `.episode`); geladen nach `qu_core::CSS` |

## Datenquelle (`moviebox.rs`, `source.rs`)

- Nur MovieBox (andere Anbieter der TUI wie 4KHDHub/BDIX nicht übernommen). Endpunkte unter `/wefeed-mobile-bff/`:
  Startseite `tab-operating?tabId=2` (Trending), Suche `subject-api/search/v2` (POST), Titel `subject-api/get`
  (+ `season-info` bei Serien), Streams `get` (`resourceDetectors`) + `play-info/v2` + `resource/v2` (+ `resource-position/v2`
  für Hinweis-Clips), Untertitel `get-ext-captions`.
- Bereiche sind fest (`SECTIONS`): kein Entfernen, `open_link` (Strg+V) gibt einen Fehler.
- IDs: Titel = `subjectId`; Episode = `<subjectId>/<staffel>/<folge>`, Film = `/0/0`. Folgen aller Staffeln
  fortlaufend nummeriert (Fortschritt/„nächste“ zählen über Staffelgrenzen), Kachel-Titel „S1 E3“.
- Stream-Wahl: direkte Dateien (eine je Auflösung, Header nur User-Agent); DASH aus play-info (Manifest aus dem
  CloudFront-Cookie, Header Referer `sportslive.wine` + User-Agent + Cookie), wenn es keine Dateien gibt oder DASH höher
  auflöst (Serien: direkt oft nur 360p). Header → mpv per `loadfile`-Optionen (`player.rs` `header_opts`).
  Geprüft 2026-11-16: mpv spielt das DASH-Manifest mit diesen Headern.
- Untertitel (`subtitle`): `get-ext-captions` mit Kandidaten-IDs nacheinander (bis 3): Ressourcen-IDs der Dateien, der
  `resource/v2`-Liste (auch Hinweis-Clips), Stream-ID von play-info; sonst wie die TUI die Folge in der alten Liste
  `subject-api/resource` suchen (filtert nicht nach Folge, 20 pro Seite). Die Stream-ID allein findet bei Original-
  Fassungen meist nichts. Alle Sprachen (ohne Doppelte), Englisch zuerst; werden in `stream()` heruntergeladen nach `<tmp>/boxqu-subs/<nr>-<hash>/<Name>.srt`
  (zusammen höchstens 8 s) und per `sub-files` (lokale Pfadliste, „:“-getrennt – mit Adressen ginge das nicht) geladen;
  `sub-add` wäre nur blockierend (libmpv2 ohne async-Befehl). mpv nimmt den Dateinamen als Spur-Titel (Sprache bleibt
  leer), daher beobachtet `player.rs` `current-tracks/sub/title` für die Meldung „Subtitles: English“ / „Subtitles off“.
- Untertitel-Knopf (wie Tubequ): Linksklick/`c` = an/aus, Rechtsklick = Menü (Sprache, Größe, Hintergrund).
  Gespeichert in `~/.config/boxqu/subtitles` (Sprache = Spur-Name, Größe, `box`/`outline`, `on`/`off`); an/aus gilt
  auch fürs nächste Video (`Controls::apply_subs` bei `FileLoaded`). Einblendungen „Subtitles: …“ nicht in den
  ersten 1,5 s nach `play`.
- Sprachfassungen: `dubs` der Titel-Daten (`lanCode`, `lanName`, `original`, `subjectId`) → `Details::languages`.
  Jede Fassung ist ein eigener Titel (eigene ID, eigener Fortschritt).
- Titel werden beim Einlesen gesäubert (`clean_title`: ab „ [“ und „ S5“ weg); Listen ohne Doppelte gleicher Titel+Jahr
  (`push_unique`), denn Klick springt ohnehin zur bevorzugten Fassung.
- Kein DNS-Fallback (die TUI hat einen über hickory); bei DNS-Sperren von aoneroom.com ggf. nachrüsten.

## Tests – Regeln

- Standard-Prüfung: `cargo build && cargo test`.
- Live-Tests gegen MovieBox nur gezielt (`--ignored`), wenige Anfragen.
- **Der Nutzer hat die installierte App meist parallel offen** (gleiche Fensterklasse `dev.roqu.BoxQu`!).
  Fenster daher nur über die **PID des selbst gestarteten Prozesses** (`$!`) finden, nie über die Klasse;
  `kill` nur mit dieser PID. (Ein Klassen-Filter hat schon einmal die falsche Instanz fotografiert und beendet.)
- Dev-Instanz mit eigenen Daten starten: `XDG_DATA_HOME`/`XDG_CONFIG_HOME` ins Scratchpad. Sonst überschreiben
  sich beide Instanzen `progress.tsv` gegenseitig (jede schreibt ihre Liste aus dem Speicher komplett neu).
- Sichtprüfung: App mit `BOXQU_DEV=1`, Fenster per PID über `hyprctl clients -j`. **Vor `grim` prüfen, dass der
  Workspace des Fensters gerade sichtbar ist** (`hyprctl monitors -j` → `activeWorkspace`), sonst landet ein
  fremder Bildschirminhalt im Screenshot.
- Abspiel-Tests stumm: `volume` (im Test-`XDG_CONFIG_HOME`) auf 0.
- **Nie** `wtype`/Eingaben simulieren, **nie** `pkill mpv` oder breite `pkill -f`-Muster; nur `kill <pid>` der Dev-Instanz.
- Temporäre Test-Hacks danach **nachweislich** zurücksetzen (grep).

## Design-Regeln

Wie Tubequ (siehe `~/Work/tubequ/CLAUDE.md`): keine Kopfzeile, Nav-Bubble oben links, Buttons nur Icons mit Tooltip,
Laden von Inhalten = graue Platzhalter in Form des Inhalts mit Glanzstreifen (qu-core-CSS `.skeleton`, wie Tubequ): Raster `grid.rs` `ghost_card`/`set_ghost` (Breiten über `part`, keine festen Pixel), Titel-Seite `show.rs` `set_loading` (Infos, Beschreibung, Poster-Fläche, Episoden-Kacheln); sonst (Bubble, Player) Spinner = `icons::spinner()` (nie `gtk::Spinner`), keine knalligen Farben, keine festen Pixelbreiten im Raster.
Poster im Hochformat 2:3, Karten ab 170 px, höchstens 12 Spalten (im Vollbild sonst riesige Poster).

## Produktentscheidungen (so gewollt – nicht ohne Rückfrage ändern)

- Startseite: **„Continue watching“ ganz oben**, dann Watchlist, dann je Startseiten-Bereich (Trending) ein Bereich.
- Sprache: **eine Vorliebe für alle Titel** (`~/.local/share/boxqu/language`, Standard `original`), gesetzt per Klick auf einen
  Sprach-Tab. Beim Öffnen eines Titels (z. B. „… [Hindi]“ aus Trending) springt die Seite zur passenden Fassung, falls vorhanden.
- Intro/Ending **nicht** automatisch überspringen (Knopf über der Timeline, sobald es Zeiten gibt).
- Fortschritt: < 30 s nicht speichern, ≥ 90 % = fertig (Abspann läuft oft noch), beim Fortsetzen 3 s zurück.

## Bedienung (Komfort)

- Timeline-Sprung: Die Timeline hält das Ziel (`seek_target`, `seek_state` in qu-core `controls.rs`), bis das Video 1 s darüber hinaus ist. mpv meldet nach dem Sprung erst das Ziel, dann das Schlüsselbild davor (`hr-seek=no`), vorher noch die alte Position – sonst springt der Zeiger Ziel → alt → Ziel bzw. kurz zurück.

- Player-Leiste: weg nach 2 s ohne Mausbewegung (`HIDE_AFTER`). Bewegung zählt nur bei geänderter Position (qu-core `player.rs`): GTK schickt nach Layout-Änderungen Bewegungs-Ereignisse an derselben Stelle nach.
- Maus-Seitentasten (GTK-Knopf 8/9) = Zurück/Vor, überall im Fenster (`Nav::forward`, merkt nur die Seite,
  nicht deren Inhalt; neuer Weg oder Home leert „Vor“).
- Weiches Scrollen mit dem Mausrad (`smooth.rs`, an jedem ScrolledWindow): Raststufen gleiten statt zu springen; Touchpad bleibt bei GTK.
- Feinschliff (qu-core-CSS, Ende): Vorschaubilder mit grauem Platzhalter (`.thumb-box`), Einblenden beim Laden (`.fade` → `.loaded`), Hover-Zoom 4 %; Ansehen-Seite blendet ein (`page-in-a/-b` im Wechsel, sonst startet die Animation nicht neu), Öffnen und Miniplayer: Zoom-Animation (`fly.rs`).
- Ton: Stummschalten blendet über den Filter `@fade` (lavfi `volume`, per `af-command`) in 160 ms aus/ein, `+`/`-` ändern die Lautstärke in kleinen Schritten (`Controls::toggle_mute`/`step_volume`). `af-command` meldet trotzdem einen Fehler (Ziel „all“ trifft auch Filter ohne den Befehl), wirkt aber.
- Tastatur im Raster: Pfeiltasten wählen die erste Karte, danach navigiert GTK; die Liste gleitet zur gewählten Karte (`smooth.rs`, nur bei Tastatur-Fokus). Timeline braucht nichts: mpv meldet `time-pos` jedes Bild.
- Miniplayer: Verlässt man die Ansehen-Seite, während ein Video läuft, spielt es unten rechts klein weiter (400 px; Knöpfe als Bubbles in den Ecken: oben links groß machen, oben rechts schließen, unten links Play + Ton). Tasten im Miniplayer: Leertaste/`k` Pause, `m` Ton, `f` zurück zum großen Video (kein Vollbild), `q` schließen; Pfeile/Ziffern bleiben fürs Raster. Die Ansehen-Seite liegt dafür als Ebene über dem Stack (im Stack nur ein Platzhalter „watch“) und wird nur verkleinert (`WatchFrame::set_mini`, `Nav::set_mini` in qu-core): der Player wird **nicht umgehängt**, sonst baut GTK die GLArea neu und mpv schaltet das Video ab. Am Ende/bei Fehler stoppt das Video (`Player::active`). Die Mini-Knöpfe erscheinen erst nach der Schrumpf-Animation (`Zoom::after`), sonst würden sie mit hochskaliert gezeichnet. Der Miniplayer lässt sich ziehen (`GestureDrag` an der Ansehen-Seite, Capture, ab 8 px; gezeichnet über `Zoom::set_offset`) und gleitet beim Loslassen (Ziehen in qu-core `nav.rs`) mit der Zoom-Animation in die nächste Ecke (`WatchFrame::set_corner`, gilt für die Sitzung). Klick ins Miniplayer-Video pausiert deshalb nicht (qu-core qu-core `player.rs` `in_mini`), Pause über den Knopf.
