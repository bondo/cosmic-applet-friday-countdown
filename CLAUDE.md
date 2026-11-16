# CLAUDE.md

A COSMIC panel applet (Rust, libcosmic) that shows nothing except on Fridays,
where it counts the minutes down to 2 PM (`TARGET_HOUR`) and then shows 🍺.
Behavior and install docs are in [README.md](README.md).

**Only NixOS is supported.** Don't add instructions, scripts or code paths for
other distros.

## Layout

- `src/main.rs`: entry point (`cosmic::applet::run`).
- `src/app.rs`: everything else.
  - `AppModel`, which implements `cosmic::Application`.
  - One subscription, `tick_stream`. It ticks at every wall-clock minute
    boundary and on every logind `PrepareForSleep` signal (before suspend and
    on wake; the argument is ignored). A signal also restarts the minute
    sleep, which runs on the monotonic clock and so doesn't count suspended
    time.
  - Functions that take their clock, D-Bus connection or event stream as
    parameters, so tests can fake them (see their doc comments), and the
    pure time logic (`label`, `until_next_minute`).
  - The unit tests, at the bottom.
- `data/`: the desktop entry and the symbolic icon (hicolor layout under
  `data/icons/`). Their file names are `APP_ID`
  (`dk.bjarkebjarke.CosmicAppletFridayCountdown`), the icon with `-symbolic`.
  A test checks this.
- `flake.nix`: the package (`packages.default`, built with nixpkgs'
  `libcosmicAppHook` and a minimal copy of the pinned toolchain, runs the
  tests), `checks` (package, clippy, rustfmt, nixfmt), `formatter`, and the
  dev shell (the package's inputs plus the full toolchain and tarpaulin).

## Commands

Run the cargo commands inside the dev shell (`nix develop`; direnv's
`use flake` does this automatically):

```sh
cargo test
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check        # hard tabs (rustfmt.toml); style edition follows Cargo.toml's 2024
nix fmt                  # formats flake.nix (nixfmt)
nix flake check          # package build + tests, clippy, rustfmt, nixfmt
```

All of these must pass before a change is done.

`nix build` and `nix flake check` only see files git knows about, so
`git add -N` new files first.

Coverage: `cargo tarpaulin --out Stdout`. Use it to find untested behavior,
not as a target: don't add tests that only execute lines. `main` and
trivial callbacks only libcosmic's runtime calls may stay uncovered, and
tarpaulin misattributes a line or two of loops.

## Rules

- **Every behavior change is covered by a test.**
  - Keep logic in pure functions, generic over `chrono::TimeZone`. Tests use
    fixed `chrono_tz` zones (or the made-up `FallsBackTwoHours` zone for a DST
    overlap), not `Local`, so they don't depend on the machine's timezone.
  - Derive expected labels from `TARGET_HOUR` (the label tests do), so
    changing it doesn't break the tests.
  - Test async and D-Bus code by passing in fake sources, as `logind_ticks`,
    `ticks` and `sleep_signals` take them. `fake_logind` is a peer-to-peer D-Bus
    connection that plays the bus and logind: it answers `GetNameOwner` for
    `org.freedesktop.login1`, and `send_prepare_for_sleep` sends
    `PrepareForSleep` from that owner, so a wrong service, path, interface or
    member fails a test.
  - Run stream tests through `paused` (tokio's clock paused) or `real_time`
    (for real IO such as D-Bus). Both fail a test that takes more than 10 s
    of real time instead of hanging the suite.
  - `tick_stream_yields_a_minute_tick` is the one test that may touch the
    machine's system bus. It is written to pass with or without one (e.g. in
    the Nix build sandbox), and runs `paused` because it waits for nothing
    from logind.
  - Test what `view` shows with `shown_texts`, which lays the view out with
    a headless renderer and collects its texts.
- Check that a new test fails without the fix, e.g. by reverting the fix in a
  scratch copy. Give the copy its own `CARGO_TARGET_DIR`: sharing one with the
  real repo can make cargo run a stale test binary.
- Keep README.md in sync when behavior, install steps or the layout change.
- **Time logic:**
  - Count real elapsed time, not wall-clock differences, so DST is handled.
  - Round partial minutes up (to the nanosecond).
  - Keep sleep durations strictly positive (leap seconds are clamped).
- **libcosmic**: follow the comment on it in `Cargo.toml` (no `rev =`, only
  the `applet` feature). Update it deliberately with
  `cargo update -p libcosmic`. The `iced_wgpu` crate still gets compiled
  through libcosmic's `wayland` feature; that's expected.
- Compare API usage against pop-os/cosmic-applets (especially
  `cosmic-applet-time`) and <https://github.com/pop-os/cosmic-applet-template>.
- Downstream NixOS configs pin a commit of this repo, so a change only reaches
  them after it is pushed and their pin is bumped.
