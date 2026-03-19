fn main() {
    hyprwire_scanner::configure()
        .compile(&["examples/basic/protocol-v1.xml"])
        .unwrap();
}
