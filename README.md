# cosmic-applet-friday-countdown

A tiny panel applet for the COSMIC desktop (System76's `cosmic-panel` / `cosmic-comp`).

**Behavior**

- Invisible every day except Friday.
- On Friday, before 2 PM local time: shows the minutes remaining, e.g. `(37 mins)`.
- On Friday, at/after 2 PM local time: shows 🍺.

It re-checks the clock at the top of every minute.

## Project layout

```
cosmic-applet-friday-countdown/
├── Cargo.toml
├── data/
│   └── dk.bjarkebjarke.CosmicAppletFridayCountdown.desktop
└── src/
    ├── main.rs   # entry point (cosmic::applet::run)
    └── app.rs    # AppModel: state, update, view, subscription
```

## Build

Requires Rust **1.93 or newer** (libcosmic's current `rust-version`) and the
system libraries COSMIC apps need (Wayland, `libxkbcommon`, fontconfig,
expat, etc.).

**Debian/Ubuntu/Pop!\_OS:**

```sh
sudo apt install pkg-config libxkbcommon-dev libwayland-dev \
    libfontconfig1-dev libfreetype-dev libexpat1-dev libssl-dev
cargo build --release
```

**Fedora:**

```sh
sudo dnf install pkgconf-pkg-config libxkbcommon-devel wayland-devel \
    fontconfig-devel freetype-devel expat-devel openssl-devel
cargo build --release
```

**NixOS:** system libraries aren't on the default search path, so use the
provided `shell.nix` (or `flake.nix`) to get `pkg-config` pointed at them:

```sh
nix-shell        # or: nix develop
cargo build --release
```

(If you build outside the shell — e.g. from an editor's integrated
terminal — make sure it's launched from within `nix-shell`/`nix develop`
too, or `pkg-config` won't see the libs.)

Check `rustc --version` inside the shell is actually ≥ 1.93 — `shell.nix`
pulls `rustc`/`cargo` from whatever `<nixpkgs>` your system channel points
at, and a channel pinned to a stable NixOS release (rather than
`nixos-unstable`) can easily lag behind libcosmic's `rust-version`. If it's
too old, prefer `flake.nix` here (pinned to `nixos-unstable`), or add a
`rust-overlay`-based toolchain to `shell.nix`.

## Install

**NixOS**

The build works fine, but the plain binary won't _run_ outside a Nix shell:
`winit`'s Wayland backend loads `libwayland-client.so` at runtime via
`dlopen`, and on NixOS that's only reachable through `LD_LIBRARY_PATH` while
`nix-shell`/`nix develop` is active. cosmic-panel has no idea `shell.nix`
exists, so it spawns the raw binary and it crashes instantly with
`NoWaylandLib`. The fix: install the real binary under a `-bin` suffix, and
put a small wrapper script at the name cosmic-panel actually calls, which
re-enters the shell.nix environment before exec'ing it.

```sh
cargo build --release

# 1. Real binary, renamed so it's never called directly
install -Dm755 target/release/cosmic-applet-friday-countdown \
    ~/.local/bin/cosmic-applet-friday-countdown-bin

# 2. shell.nix needs to live somewhere permanent — the wrapper references
#    this exact path every time it runs
mkdir -p ~/.local/share/cosmic-applet-friday-countdown
cp shell.nix ~/.local/share/cosmic-applet-friday-countdown/

# 3. Wrapper script becomes the actual `cosmic-applet-friday-countdown`
#    that cosmic-panel and the .desktop file's Exec= line both call
install -Dm755 scripts/cosmic-applet-friday-countdown-wrapper.sh \
    ~/.local/bin/cosmic-applet-friday-countdown

# 4. Desktop entry, pointed at the wrapper
mkdir -p ~/.local/share/applications
install -Dm644 data/dk.bjarkebjarke.CosmicAppletFridayCountdown.desktop \
    ~/.local/share/applications/dk.bjarkebjarke.CosmicAppletFridayCountdown.desktop
sed -i "s|^Exec=.*|Exec=$HOME/.local/bin/cosmic-applet-friday-countdown|" \
    ~/.local/share/applications/dk.bjarkebjarke.CosmicAppletFridayCountdown.desktop

killall cosmic-panel
```

Sanity-check the wrapper directly before trusting the panel with it:

```sh
~/.local/bin/cosmic-applet-friday-countdown
```

It'll pause for a second or two the first time (`nix-shell` resolving the
environment) — expected, and it only adds a small delay to the applet's
startup, not to its normal operation afterward.

**Better long-term option:** if you want to avoid the `nix-shell` startup
cost and the two-path indirection entirely, package this properly as a Nix
derivation (`buildRustPackage` + `autoPatchelfHook` or `wrapProgram`) so the
library paths are baked into the binary's RPATH at build time. Happy to put
that together if you'd rather go that route than maintain the wrapper.

**Other distros**

```sh
sudo install -Dm755 target/release/cosmic-applet-friday-countdown \
    /usr/bin/cosmic-applet-friday-countdown

sudo install -Dm644 data/dk.bjarkebjarke.CosmicAppletFridayCountdown.desktop \
    /usr/share/applications/dk.bjarkebjarke.CosmicAppletFridayCountdown.desktop
```

Then either restart `cosmic-panel` (e.g. `killall cosmic-panel`) or log out and back in, and add it from
**Settings → Desktop → Panel → Applets → Add Applet**, searching for
"Friday 2PM Countdown".

## Notes / things you may want to tweak

- **Target time**: change `TARGET_HOUR` in `src/app.rs` (24h clock, local time).
- **Update timing**: the subscription wakes up exactly at the top of each
  minute (see `subscription()` in `src/app.rs`), the same way
  cosmic-applet-time's own clock does, so both update in the same instant
  rather than drifting apart on separate fixed-interval polls.
- **Timezone**: uses `chrono::Local`, i.e. whatever timezone the system is set to.
- **Day check**: uses `Weekday::Fri` from `chrono`, so it follows the system
  locale's calendar, not a custom definition of "Friday."
- No popup is defined since the applet is display-only — if you want clicking
  it to open a popup (e.g. showing next Friday's date), the `Application`
  trait also supports `view_window`/popup handling like other COSMIC applets;
  happy to add that if useful.
- I built this against the current `pop-os/libcosmic` `applet` API
  (`Core`/`Task`/`Application` trait, `core.applet.text(...)`). Since
  libcosmic is pre-1.0 and pulled straight from git, a `cargo build` may
  need small tweaks if the API has shifted since — `cargo generate
gh:pop-os/cosmic-applet-template` is a good source of truth to diff against.
