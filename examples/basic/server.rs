mod test_protocol_v1 {
    include!(concat!(env!("OUT_DIR"), "/test_protocol_v1.rs"));
}

use hyprwire::server;
use std::str::FromStr;
use std::{env, path};

fn socket_path() -> path::PathBuf {
    let mut runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap();
    runtime_dir.push_str("/test-hw.sock");

    path::PathBuf::from_str(&runtime_dir).unwrap()
}

fn main() {
    let path = socket_path();
    let server = server::Server::open(Some(&path)).unwrap();
}
