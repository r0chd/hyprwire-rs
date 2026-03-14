mod test_protocol_v1 {
    include!(concat!(env!("OUT_DIR"), "/test_protocol_v1.rs"));
}

use hyprwire::client;
use hyprwire::implementation::client::ProtocolImplementations;
use hyprwire::implementation::types::ProtocolSpec;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
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
                println!("Server says {}", message.to_string_lossy());
            }
            test_protocol_v1::client::MyManagerV1Event::RecvMessageArrayUint { message } => {
                println!("Server sent uint array {:?}", message);
            }
            _ => {}
        }
    }
}

impl hyprwire::Dispatch<test_protocol_v1::client::MyObjectV1Object> for App {
    fn event(
        &mut self,
        _proxy: &test_protocol_v1::client::MyObjectV1Object,
        event: test_protocol_v1::client::MyObjectV1Event,
    ) {
        if let test_protocol_v1::client::MyObjectV1Event::SendMessage { message } = event {
            println!("Server says on object {}", message.to_string_lossy());
        }
    }
}

fn main() {
    env_logger::Builder::new()
        .filter(None, log::LevelFilter::Trace)
        .init();

    let path = socket_path();
    let mut socket = client::Client::open(&path);

    let implementation = test_protocol_v1::client::TestProtocolV1Impl::default();
    socket.add_implementation(implementation);
    socket.wait_for_handshake().unwrap();

    let spec = socket
        .get_spec(implementation.protocol().spec_name())
        .unwrap();

    println!(
        "test protocol supported at version {}. Binding.",
        spec.spec_ver()
    );

    let mut state = App::default();

    let obj = socket.bind_protocol(implementation.protocol(), 1).unwrap();
    let obj = hyprwire::implementation::types::Object::from_raw(obj);
    let manager = test_protocol_v1::client::MyManagerV1Object::new(obj, &mut state);

    println!("Bound!");

    let mut pipes = UnixStream::pair().unwrap();
    let buf = b"pipe!";
    pipes.1.write_all(buf).unwrap();

    println!("Will send fd {}\n", pipes.0.as_raw_fd());

    let mut pipes2 = UnixStream::pair().unwrap();
    let mut pipes3 = UnixStream::pair().unwrap();

    let buf = b"o kurwa";
    pipes2.1.write_all(buf).unwrap();
    let buf = b"bober!!";
    pipes3.1.write_all(buf).unwrap();

    manager.send_send_message("Hello!");
    manager.send_send_message_fd(pipes.0.as_raw_fd());
    manager.send_send_message_array_fd(&[pipes2.0.as_raw_fd(), pipes3.0.as_raw_fd()]);
    manager.send_send_message_array(&["Hello", "via", "array!"]);
    manager.send_send_message_array(&[]);
    manager.send_send_message_array_uint(&[69, 420, 1337]);

    socket.roundtrip().unwrap();

    let obj = manager.send_make_object().unwrap();
    let obj = test_protocol_v1::client::MyObjectV1Object::new(obj, &mut state);

    obj.send_send_message("Hello on object");
    obj.send_send_enum(test_protocol_v1::spec::MyEnum::World);

    while socket.dispatch_events(true).is_ok() {}
}
