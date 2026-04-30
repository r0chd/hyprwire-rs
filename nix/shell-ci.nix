{ pkgs, rustToolchain }:
pkgs.mkShell {
  name = "hyprwire-rs-ci-devshell";
  packages = builtins.attrValues {
    inherit
      rustToolchain
      ;
    inherit (pkgs)
      curl
      cargo-audit
      cargo-deny
      cargo-udeps
      grcov
      codecov-cli
      ;
  };

  RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
}
