use hyprwire_scanner::{Targets, generate, parse};
use insta::assert_snapshot;
use std::fs;

#[test]
fn test_scanner_protocol_v1() {
    let xml = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/protocol-v1.xml"
    ))
    .unwrap();
    let protocol = parse::parse_protocol(&xml).unwrap();
    let code = generate::generate(&protocol, Targets::ALL, &[]);
    assert_snapshot!(code);
}

#[test]
fn test_scanner_protocol_v1_client_only() {
    let xml = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/protocol-v1.xml"
    ))
    .unwrap();
    let protocol = parse::parse_protocol(&xml).unwrap();
    let code = generate::generate(&protocol, Targets::CLIENT, &[]);

    assert!(code.contains("pub mod client"));
    assert!(code.contains("mod spec"));
    assert!(!code.contains("pub mod spec"));
    assert!(!code.contains("pub mod server"));
}

#[test]
fn test_scanner_protocol_v1_server_only() {
    let xml = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/protocol-v1.xml"
    ))
    .unwrap();
    let protocol = parse::parse_protocol(&xml).unwrap();
    let code = generate::generate(&protocol, Targets::SERVER, &[]);

    assert!(code.contains("pub mod server"));
    assert!(code.contains("mod spec"));
    assert!(!code.contains("pub mod spec"));
    assert!(!code.contains("pub mod client"));
}

#[test]
fn test_scanner_protocol_v1_derive_macro() {
    let xml = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/protocol-v1.xml"
    ))
    .unwrap();
    let protocol = parse::parse_protocol(&xml).unwrap();
    let code = generate::generate(
        &protocol,
        Targets::ALL,
        &[(".".to_string(), "#[derive(serde::Deserialize)]".to_string())],
    );
    assert_snapshot!(code);
}
