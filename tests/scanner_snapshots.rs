use hyprwire::scanner::{generate, parse};
use insta::assert_snapshot;
use std::fs;

#[test]
fn test_scanner_protocol_v1() {
    let xml = fs::read_to_string("tests/protocol-v1.xml").unwrap();
    let protocol = parse::parse_protocol(&xml).unwrap();
    let code = generate::generate(&protocol);
    assert_snapshot!(code);
}
