fn main() {
    hyprwire_scanner::configure()
        .compile("examples/protocols/protocol-v1.xml")
        .unwrap();
}
