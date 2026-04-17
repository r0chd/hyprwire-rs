fn main() {
    hyprwire_scanner::configure()
        .with_targets(hyprwire_scanner::Targets::CLIENT | hyprwire_scanner::Targets::SERVER)
        .compile(&[
            "examples/protocols/protocol-v1.xml",
            "benches/protocols/bench-protocol-v1.xml",
        ])
        .unwrap();
}
