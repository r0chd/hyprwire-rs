use hyprwire_scanner::{generate, parse};
use std::{env, fs, path};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    let xml = fs::read_to_string("examples/basic/protocol-v1.xml")
        .expect("failed to read protocol-v1.xml");
    let protocol = parse::parse_protocol(&xml).expect("failed to parse protocol XML");
    let code = generate::generate(&protocol);

    let out_path = path::Path::new(&out_dir).join("test_protocol_v1.rs");
    fs::write(&out_path, code).expect("failed to write generated file");

    println!("cargo::rerun-if-changed=examples/basic/protocol-v1.xml");
}
