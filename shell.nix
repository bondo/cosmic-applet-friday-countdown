# Run with: nix-shell
#
# Provides the native libs that libcosmic (via smithay-client-toolkit,
# iced/wgpu, and friends) needs at build time. Without these, pkg-config
# can't find things like xkbcommon.pc, wayland-client.pc, etc.
{
  pkgs ? import <nixpkgs> { },
}:

pkgs.mkShell rec {
  # nativeBuildInputs = with pkgs; [
  #   pkg-config
  #   rustc
  #   cargo
  # ];

  buildInputs = with pkgs; [
    # Wayland + input (smithay-client-toolkit)
    wayland
    libxkbcommon

    # GPU/rendering (wgpu backend)
    libGL
    vulkan-loader

    # Font/text stack
    expat
    fontconfig
    freetype

    # X11 libs some deps still probe for even on Wayland
    libx11
    libxcursor
    libxi
    libxrandr

    # cosmic-applet-friday-countdown itself doesn't need this, but most
    # libcosmic deps in the tree pull in openssl transitively
    openssl
  ];

  # So the built binary can find the dynamic libs at *runtime* too,
  # not just link against them at build time.
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
}
