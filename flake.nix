{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
    in
    {
      devShells = forAllSystems (
        pkgs:
        let
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "rust-analyzer"
              "rust-src"
            ];
          };
        in
        {
          default = pkgs.mkShell (
            pkgs.lib.fix (finalAttrs: {
              buildInputs = builtins.attrValues {
                inherit (pkgs)
                  cargo-insta
                  cargo-audit
                  cargo-deny
                  cargo-udeps
                  nixd
                  ;
                inherit rustToolchain;
              };

              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath finalAttrs.buildInputs;
              RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            })
          );
        }
      );

      formatter = forAllSystems (
        pkgs:
        let
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "rust-analyzer"
              "rust-src"
            ];
          };
        in
        pkgs.writeShellApplication {
          name = "nix3-fmt-wrapper";
          runtimeInputs = builtins.attrValues {
            inherit (pkgs)
              nixfmt
              taplo
              fd
              ;
            inherit rustToolchain;
          };
          text = ''
            fd "$@" -t f -e nix -x nixfmt -q '{}'
            fd "$@" -t f -e toml -x taplo format '{}'
            cargo fmt
          '';
        }
      );
    };
}
