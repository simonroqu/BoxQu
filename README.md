# BoxQu

A lightweight movie and series player for Linux, written in Rust with GTK4 and libmpv.
Same look and feel as AniQu and [Tube](https://github.com/simonroqu/tube).

A desktop movie and series player for MovieBox: poster grid, search and an embedded player.
Standalone, no other tools needed besides GTK and mpv.

- Home page with Continue watching, your watchlist and MovieBox trending
- Search, title pages with all seasons and episodes (a movie is one), watchlist
- Language versions as tabs (Original, Hindi, …); your choice applies to every title, Original by default
- Embedded mpv player with its own controls, subtitles and quality switch
- Continue watching per episode, playback speed per title
- Follows your [Omarchy](https://omarchy.org) theme live, ships its own default theme otherwise

## Requirements

- GTK ≥ 4.20
- mpv / libmpv
- Optional: `mpv-mpris` for media keys

## Build & install

```sh
cargo build --release
./install.sh        # installs to ~/.local/bin/boxqu and adds a launcher entry
```

Run directly:

```sh
boxqu                    # home page
boxqu dune               # search
```

## Keyboard shortcuts

| Key | Action |
|---|---|
| `/`, `Ctrl+K` | Search |
| `F5`, `Ctrl+R` | Reload |
| `n` | Next episode |
| `0`–`9` | Seek to 0–90 % |
| `,` / `.` | Previous / next frame |
| `c` | Subtitles on/off (right-click the subtitle button for language, size and background) |

Double-click left/right on the player seeks −/+10 s, the middle toggles fullscreen.

## Data

- `~/.local/share/boxqu/`: progress, watchlist, preferred language, recent searches, speed per title
- `~/.config/boxqu/`: quality, volume, columns
- `~/.cache/boxqu/`: posters, last trending list

## Disclaimer

BoxQu does not host any content; it only plays what MovieBox provides. Use it in accordance with
the laws of your country and the terms of the services you access.

## Credits

- MovieBox API access (request signing, endpoints) ported from
  [MovieBox-Tui](https://github.com/mesamirh/MovieBox-Tui) (MIT/Apache-2.0)
- [mpv](https://mpv.io) for playback
- [FSRCNNX](https://github.com/igv/FSRCNN-TensorFlow) upscaling shader by igv (GPL-3.0/LGPL-3.0, via qu-core)

## License

[GPL-3.0-or-later](LICENSE)
