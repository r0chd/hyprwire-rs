{ pkgs, rustToolchain }:
pkgs.mkShell (
  pkgs.lib.fix (finalAttrs: {
    name = "hyprwire-rs-devshell";
    packages = builtins.attrValues {
      inherit
        rustToolchain
        ;
      inherit (pkgs)
        rust-analyzer-unwrapped
        cargo-insta
        nixd
        ;
    };

    RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
  })
)
