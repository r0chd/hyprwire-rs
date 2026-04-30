{
  rust-overlay ? import (import ./nix/flake-parse.nix "rust-overlay"),
  pkgs ? import (import ./nix/flake-parse.nix "nixpkgs") {
    system = builtins.currentSystem;
    overlays = [ rust-overlay ];
    config.allowUnfree = true;
  },
}:
let
  rustToolchain = pkgs.rust-bin.stable.latest.default.override {
    extensions = [
      "rust-analyzer"
      "rust-src"
      "llvm-tools"
    ];
  };
in
{
  shell = import ./nix/shell.nix {
    inherit pkgs rustToolchain;
  };

  shell-ci = import ./nix/shell-ci.nix {
    inherit pkgs rustToolchain;
  };

  formatter = import ./nix/formatter.nix { inherit pkgs rustToolchain; };
}
