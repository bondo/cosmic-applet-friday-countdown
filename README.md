# cosmic-applet-friday-countdown

A tiny panel applet for the COSMIC desktop (System76's `cosmic-panel` / `cosmic-comp`).
Only NixOS is supported.

**Behavior**

- Every day except Friday it shows nothing. The panel still reserves the
  smallest window it can (1×1 px), and may add its usual spacing around it.
- On Friday, before 2 PM local time: shows the minutes remaining, rounded up,
  e.g. `(37 mins)`.
- On Friday, at/after 2 PM local time: shows 🍺.

It updates at the top of every minute. After the machine wakes from suspend it
updates immediately and then carries on at the top of every minute.

## Project layout

```
cosmic-applet-friday-countdown/
├── flake.nix        # package, dev shell, `nix flake check`, `nix fmt`
├── data/            # desktop entry and symbolic icon
└── src/
    ├── main.rs      # entry point (cosmic::applet::run)
    └── app.rs       # AppModel, tick stream, countdown logic, tests
```

[CLAUDE.md](CLAUDE.md) is the guide for AI coding agents.

## Build

The flake's dev shell has the Rust toolchain pinned in `rust-toolchain.toml`
(libcosmic needs ≥ 1.93), the package's build inputs and `cargo-tarpaulin`.
nixpkgs' `libcosmicAppHook` sets `RUSTFLAGS` there so that cargo links the
libraries winit would otherwise `dlopen` (libwayland, libxkbcommon, …), so
`cargo run` works. With direnv, `.envrc` enters the shell automatically.
Binaries built this way are for development; for daily use, install the Nix
package (see below).

```sh
nix develop
cargo build
```

## Test

```sh
cargo test
nix flake check   # package build (runs the tests), clippy, rustfmt, nixfmt
```

## Install

Install the flake's package. It is built with nixpkgs' `libcosmicAppHook`,
which puts libwayland, libxkbcommon etc. on the binary's runtime search path,
so cosmic-panel can start it directly. It also installs the desktop entry and
icon.

```sh
nix profile add github:bondo/cosmic-applet-friday-countdown
```

Or add this flake as an input of your configuration, make `inputs` available
to your modules (e.g. via `specialArgs` for NixOS, `extraSpecialArgs` for
home-manager), and add the package. In NixOS:

```nix
environment.systemPackages = [
  inputs.cosmic-applet-friday-countdown.packages.${pkgs.stdenv.hostPlatform.system}.default
];
```

or in home-manager:

```nix
home.packages = [
  inputs.cosmic-applet-friday-countdown.packages.${pkgs.stdenv.hostPlatform.system}.default
];
```

Then either restart `cosmic-panel` (e.g. `killall cosmic-panel`) or log out
and back in, and add it from **Settings → Desktop → Panel → Applets → Add
Applet**, searching for "Friday 2 PM Countdown".

## Notes / things you may want to tweak

- **Target time**: change `TARGET_HOUR` in `src/app.rs` (24h clock, local
  time, 1–23). The tests follow it. "2 PM" also appears in the desktop entry
  (`Name`, `Comment`) and in `Cargo.toml`'s `description`; update those too.
- **Suspend**: the wake-up refresh listens for logind's `PrepareForSleep`
  signal (it also refreshes when the machine goes to sleep, which is
  harmless). Without a system D-Bus/logind it logs a warning to stderr and
  only updates on minute boundaries.
- **Timezone**: uses `chrono::Local`, i.e. the system timezone. A timezone
  change shows up at the next minute tick. Minutes are counted in real
  elapsed time, so a DST switch on a Friday morning doesn't skew the count.
- **libcosmic** comes from git, pinned by `Cargo.lock`. Update it with
  `cargo update -p libcosmic`.

## License

[MPL-2.0](LICENSE).
