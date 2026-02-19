use hyprwire::client::{ClientSocket, SocketSource};
use std::{env, path, str::FromStr};

fn socket_path() -> path::PathBuf {
    let mut runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap();
    runtime_dir.push_str("/test-hw.sock");

    path::PathBuf::from_str(&runtime_dir).unwrap()
}

fn main() {
    env_logger::init();

    let path = socket_path();
    let _client = ClientSocket::open(SocketSource::Path(&path));
}
