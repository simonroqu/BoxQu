# BoxQu

A lightweight movie and series player for Linux (and, untested, Windows and macOS), written in Rust with GTK4 and libmpv.
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

## Install

One script per platform: it installs whatever is missing (GTK, mpv, Rust), builds BoxQu and adds it to
your app launcher / start menu. The first run takes a while (downloading and compiling), later runs are quick.

### Linux

```sh
git clone https://github.com/simonroqu/BoxQu
cd BoxQu
./install.sh
```

Supports Arch, Debian/Ubuntu, Fedora and openSUSE (asks for your sudo password if packages are missing).
Needs a distribution with GTK ≥ 4.20 (e.g. Arch, Fedora 43, Ubuntu 25.10).
Installs to `~/.local/bin/boxqu` plus a launcher entry and icon.

### macOS (untested)

Needs [Homebrew](https://brew.sh). Then in Terminal:

```sh
git clone https://github.com/simonroqu/BoxQu
cd BoxQu
./install.sh
```

Installs `~/Applications/BoxQu.app` (shows up in Launchpad and Spotlight, no icon yet) and `~/.local/bin/boxqu`.

### Windows (untested)

In PowerShell:

```powershell
winget install -e --id Git.Git      # once, if git is missing (then open a new PowerShell)
git clone https://github.com/simonroqu/BoxQu
cd BoxQu
powershell -ExecutionPolicy Bypass -File install.ps1
```

The script installs [MSYS2](https://www.msys2.org) (if missing) and builds BoxQu with GTK and mpv from there.
Installs to `%LOCALAPPDATA%\Programs\BoxQu` plus a start menu entry.

### Update and uninstall

- Update: `git pull` in the folder, then run the install script again. It always builds against the latest
  [qu-core](https://github.com/simonroqu/quCore).
- Uninstall: delete the files the install script lists at the end.
- Building fetches qu-core from GitHub, so it has to be reachable from your machine.

## Usage

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
the laws of your country and the terms of the services you access. BoxQu is not affiliated with MovieBox or with any rights holder.

## Credits

- MovieBox API access (request signing, endpoints) ported from
  [MovieBox-Tui](https://github.com/mesamirh/MovieBox-Tui) (MIT/Apache-2.0)
- [mpv](https://mpv.io) for playback
- [FSRCNNX](https://github.com/igv/FSRCNN-TensorFlow) upscaling shader by igv (GPL-3.0/LGPL-3.0, via qu-core)

## License

[GPL-3.0-or-later](LICENSE)
