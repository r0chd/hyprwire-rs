mod test_protocol_v1 {
    hyprwire::include_protocol!("test_protocol_v1");
}

use hyprwire::client;
use hyprwire::implementation::client::ProtocolImplementations;
use hyprwire::implementation::types::ProtocolSpec;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::net;
use std::str::FromStr;
use std::{env, path};

fn socket_path() -> path::PathBuf {
    let mut runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap();
    runtime_dir.push_str("/test-hw.sock");

    path::PathBuf::from_str(&runtime_dir).unwrap()
}

#[derive(Default)]
struct App {}

impl hyprwire::Dispatch<test_protocol_v1::client::MyManagerV1Object> for App {
    fn event(
        &mut self,
        _proxy: &test_protocol_v1::client::MyManagerV1Object,
        event: test_protocol_v1::client::MyManagerV1Event,
    ) {
        match event {
            test_protocol_v1::client::MyManagerV1Event::SendMessage { message } => {
                println!("Server says {}", message);
            }
            test_protocol_v1::client::MyManagerV1Event::RecvMessageArrayUint { message } => {
                println!("Server sent uint array {:?}", message);
            }
        }
    }
}

impl hyprwire::Dispatch<test_protocol_v1::client::MyObjectV1Object> for App {
    fn event(
        &mut self,
        _proxy: &test_protocol_v1::client::MyObjectV1Object,
        event: test_protocol_v1::client::MyObjectV1Event,
    ) {
        let test_protocol_v1::client::MyObjectV1Event::SendMessage { message } = event;
        println!("Server says on object {}", message);
    }
}

fn main() {
    env_logger::Builder::new()
        .filter(None, log::LevelFilter::Trace)
        .init();

    let path = socket_path();
    let mut socket = client::Client::open(&path).unwrap();

    let implementation = test_protocol_v1::client::TestProtocolV1Impl::default();
    socket.add_implementation(implementation.clone());
    socket.wait_for_handshake().unwrap();

    let spec = socket
        .get_spec(implementation.protocol().spec_name())
        .unwrap();

    println!(
        "test protocol supported at version {}. Binding.",
        spec.spec_ver()
    );

    let mut state = App::default();

    let manager = socket
        .bind::<test_protocol_v1::client::MyManagerV1Object, App>(implementation.protocol(), 1)
        .unwrap();

    println!("Bound!");

    let mut pipes = net::UnixStream::pair().unwrap();
    let buf = b"pipe!";
    pipes.1.write_all(buf).unwrap();
    drop(pipes.1);

    println!("Will send fd {}\n", pipes.0.as_raw_fd());

    let mut pipes2 = net::UnixStream::pair().unwrap();
    let mut pipes3 = net::UnixStream::pair().unwrap();

    pipes2.1.write_all(b"o kurwa").unwrap();
    pipes3.1.write_all(b"bober!!").unwrap();
    drop(pipes2.1);
    drop(pipes3.1);

    manager.send_send_message("Hello!");
    manager.send_send_message_fd(&pipes.0);
    manager.send_send_message_array_fd(&[&pipes2.0, &pipes3.0]);
    manager.send_send_message_array(&["Hello", "via", "array!"]);
    manager.send_send_message_array::<&str>(&[]);
    manager.send_send_message_array_uint(&[69, 420, 1337]);

    socket.roundtrip(&mut state).unwrap();

    let obj = manager
        .send_make_object::<test_protocol_v1::client::MyObjectV1Object, App>()
        .unwrap();

    obj.send_send_message("Hello on object");
    obj.send_send_enum(test_protocol_v1::spec::MyEnum::World);

    loop {
        if let Err(e) = socket.dispatch_events(&mut state, true) {
            log::error!("{e}");
            break;
        }
    }
}
