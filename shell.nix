{
  pkgs ? import <nixpkgs> { },
}:

pkgs.mkShell rec {
  runtimeInputs = with pkgs; [
    wayland
  ];

  # So the built binary can find the dynamic libs at *runtime* too,
  # not just link against them at build time.
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeInputs;
}
