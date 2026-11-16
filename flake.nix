{
  description = "cosmic-applet-friday-countdown";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      # Linux systems: only NixOS is supported.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = nixpkgs.lib.genAttrs systems;
      perSystem = forEachSystem (
        system:
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs {
            inherit system overlays;
          };
          lib = pkgs.lib;
          # nixpkgs' own rustc can lag behind libcosmic's `rust-version`, so
          # use the toolchain pinned in rust-toolchain.toml: in full for the dev
          # shell, and only with clippy and rustfmt (for the checks below) for
          # building.
          rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          # rust-toolchain.toml's channel must be an exact stable version
          # (e.g. "1.96.0"), not "stable" or a nightly.
          rustVersion = (lib.importTOML ./rust-toolchain.toml).toolchain.channel;
          buildRust = pkgs.rust-bin.stable.${rustVersion}.minimal.override {
            extensions = [
              "clippy"
              "rustfmt"
            ];
          };
          rustPlatform = pkgs.makeRustPlatform {
            cargo = buildRust;
            rustc = buildRust;
          };
          cargoToml = lib.importTOML ./Cargo.toml;

          package = rustPlatform.buildRustPackage {
            pname = cargoToml.package.name;
            version = cargoToml.package.version;

            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./src
                ./data
              ];
            };

            cargoLock = {
              lockFile = ./Cargo.lock;
              # libcosmic and friends are git dependencies; fetch them at the
              # revisions Cargo.lock pins instead of maintaining outputHashes.
              allowBuiltinFetchGit = true;
            };

            # Puts libwayland, libxkbcommon, … on the binary's runtime search
            # path, so cosmic-panel can launch it directly.
            nativeBuildInputs = [ pkgs.libcosmicAppHook ];

            postInstall = ''
              install -Dm644 data/*.desktop -t $out/share/applications
              mkdir -p $out/share/icons/hicolor
              cp -r data/icons/. $out/share/icons/hicolor/
            '';

            meta = {
              description = cargoToml.package.description;
              homepage = "https://github.com/bondo/cosmic-applet-friday-countdown";
              license = lib.licenses.mpl20;
              mainProgram = cargoToml.package.name;
              platforms = lib.platforms.linux;
            };
          };

          # Same sources and vendored dependencies as the package, but runs
          # clippy instead of building and testing.
          clippy = package.overrideAttrs {
            pname = "${cargoToml.package.name}-clippy";
            doCheck = false;
            buildPhase = ''
              runHook preBuild
              cargo clippy --all-targets --locked --offline -- -D warnings
              runHook postBuild
            '';
            installPhase = "touch $out";
            dontFixup = true;
          };

          rustfmt =
            pkgs.runCommand "${cargoToml.package.name}-rustfmt" { nativeBuildInputs = [ buildRust ]; }
              ''
                cd ${
                  lib.fileset.toSource {
                    root = ./.;
                    fileset = lib.fileset.unions [
                      ./Cargo.toml
                      ./rustfmt.toml
                      ./src
                    ];
                  }
                }
                HOME=$TMPDIR cargo fmt --check
                touch $out
              '';

          nixfmt =
            pkgs.runCommand "${cargoToml.package.name}-nixfmt" { nativeBuildInputs = [ pkgs.nixfmt ]; }
              ''
                nixfmt --check ${./flake.nix}
                touch $out
              '';
        in
        {
          packages.default = package;

          # `nix flake check`: build + tests (buildRustPackage runs `cargo test`
          # in its check phase), clippy, rustfmt and nixfmt.
          checks = {
            default = package;
            inherit clippy rustfmt nixfmt;
          };

          # `nix fmt` formats every .nix file (nixfmt via treefmt).
          formatter = pkgs.nixfmt-tree;

          devShells.default = pkgs.mkShell {
            # The package's build inputs. libcosmicAppHook's shell hook also
            # makes cargo link the libraries winit and iced would dlopen()
            # (libwayland, libxkbcommon, ...), so `cargo run` works.
            inputsFrom = [ package ];
            packages = [
              rust
              pkgs.cargo-tarpaulin
            ];
          };
        }
      );
      # `perSystem.<system>.<output>` as `<output>.<system>`.
      pick = output: nixpkgs.lib.mapAttrs (_: outputs: outputs.${output}) perSystem;
    in
    {
      packages = pick "packages";
      checks = pick "checks";
      formatter = pick "formatter";
      devShells = pick "devShells";
    };
}
