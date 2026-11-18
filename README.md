# BoxQu

A lightweight movie and series player for Linux (and, untested, Windows and macOS), written in Rust with GTK4 and libmpv.
Same look and feel as AniQu and [TubeQu](https://github.com/simonroqu/tubequ).

A desktop movie and series player for MovieBox: poster grid, search and an embedded player.
Standalone, no other tools needed besides GTK and mpv.

- Home page with Continue watching, your watchlist and MovieBox trending
- Search, title pages with all seasons and episodes (a movie is one), watchlist
- Language versions as tabs (Original, Hindi, …); your choice applies to every title, Original by default
- Embedded mpv player with its own controls, subtitles and quality switch
- Continue watching per episode, playback speed per title
- Follows your [Omarchy](https://omarchy.org) theme live, ships its own default theme otherwise
- Android app for phones and Android TV (build it yourself, see [Android](#android-phone-and-android-tv))

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

### Android (phone and Android TV)

There is no ready-made APK to download: one script builds it from the same folder. Home page sections, search, title pages with language tabs, movies and series, subtitles, continue watching.

Works on phones and on Android TV (shows up in the TV launcher, fully usable with the remote). Needs Android 8.0 or newer.

**Linux and macOS (macOS untested):**

```sh
./build-apk.sh
```

**Windows (untested):** in PowerShell, in the cloned folder:

```powershell
powershell -ExecutionPolicy Bypass -File build-apk.ps1
```

Windows builds through [WSL](https://learn.microsoft.com/windows/wsl/) (Linux inside Windows) with the same script.
If WSL isn't set up yet, the first run sets it up with Ubuntu: it asks for administrator rights, then **restart Windows**,
open "Ubuntu" from the start menu once (pick a user name and password) and run the script again. Needs Windows 10
(version 2004 or newer) or Windows 11 with virtualization enabled; company PCs sometimes block WSL.

**What the script does:** it sets up everything that's missing (a JDK, the Android SDK and NDK in `~/Android/Sdk`,
Rust with the Android targets in `~/Android/rust` — your normal Rust stays untouched — and cargo-ndk), then builds
`target/BoxQu.apk`. The first run downloads a few GB and takes a while, later runs are quick. It asks for your password
only to install system packages (JDK, unzip, a C compiler).

The APK is **not** stored in the repository and does **not** update by itself: after every `git pull`, run the script again.

**Install on the device:**

- If exactly one device is connected with USB or wireless debugging (developer options), the script installs the APK
  right away. On Windows this needs adb: `winget install -e --id Google.PlatformTools`.
- Otherwise copy `target/BoxQu.apk` to the device (USB, cloud, a USB stick for TVs), open it and allow installing apps
  from that file manager.
- Android TV with adb: enable developer options and network debugging on the TV, then
  `adb connect <tv-ip>:5555` and `adb install -r target/BoxQu.apk`.

**Updates:** install the new APK over the old one, your data stays. The APK is signed with a debug key that is
created per computer: an APK built on a different computer can't update the installed one, uninstall first
(this deletes progress and lists).

### Update and uninstall

- Update: `git pull` in the folder, then run the install script (and `build-apk` for Android) again. It always builds against the latest
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

Paths for Linux and macOS; on Windows the same files are in `%APPDATA%\boxqu` and `%LOCALAPPDATA%\boxqu` (cache).
On Android everything stays inside the app (uninstalling deletes it).

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
