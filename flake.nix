{
  description = "cosmic-applet-friday-countdown";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rust = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
          targets = [ pkgs.stdenv.hostPlatform.config ];
        };
      in
      {
        devShells.default =
          with pkgs;
          mkShell {
            buildInputs = [
              wayland
              libxkbcommon
              libGL
              vulkan-loader
              expat
              fontconfig
              freetype
              libx11
              libxcursor
              libxi
              libxrandr
              openssl
              pkg-config
              rust
            ];
          };
      }
    );
}
