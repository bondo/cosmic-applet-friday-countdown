#!/usr/bin/env bash
# On NixOS, libwayland-client.so, libxkbcommon.so, etc. aren't on a global
# runtime search path — they only become visible via LD_LIBRARY_PATH inside
# a nix-shell. cosmic-panel spawns applets without that environment, so the
# real binary (renamed to *-bin below) crashes with `NoWaylandLib` when run
# directly. This wrapper re-enters the shell.nix environment before exec'ing
# the real binary, so it works the same whether launched by cosmic-panel or
# by hand.
set -euo pipefail

SHELL_NIX="$HOME/.local/share/cosmic-applet-friday-countdown/shell.nix"
REAL_BIN="$HOME/.local/bin/cosmic-applet-friday-countdown-bin"

exec nix-shell "$SHELL_NIX" --run "exec \"$REAL_BIN\""
