mod test_protocol_v1;

use hyprwire::client;
use hyprwire::implementation::client::ProtocolImplementations;
use hyprwire::implementation::types::ProtocolSpec;
use std::{
    env,
    io::Write,
    os::{fd::AsRawFd, unix::net::UnixStream},
    path,
    str::FromStr,
};

fn socket_path() -> path::PathBuf {
    let mut runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap();
    runtime_dir.push_str("/test-hw.sock");

    path::PathBuf::from_str(&runtime_dir).unwrap()
}

#[derive(Default)]
struct App {}

impl test_protocol_v1::client::Dispatch<test_protocol_v1::client::MyManagerV1Object> for App {
    fn event(&mut self, event: test_protocol_v1::client::MyManagerV1Event) {
        match event {
            test_protocol_v1::client::MyManagerV1Event::SendMessage { message } => {
                println!("Server says {}", message.to_string_lossy());
            }
            test_protocol_v1::client::MyManagerV1Event::RecvMessageArrayUint { message } => {
                println!("Server sent uint array {:?}", message);
            }
        }
    }
}

impl test_protocol_v1::client::Dispatch<test_protocol_v1::client::MyObjectV1Object> for App {
    fn event(&mut self, event: test_protocol_v1::client::MyObjectV1Event) {
        match event {
            test_protocol_v1::client::MyObjectV1Event::SendMessage { message } => {
                println!("Server says on object {}", message.to_string_lossy());
            }
        }
    }
}

fn main() {
    env_logger::init();

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

    manager.send_send_message(b"Hello!");
    manager.send_send_message_fd(pipes.0.as_raw_fd());
    manager.send_send_message_array_fd(&[pipes2.0.as_raw_fd(), pipes3.0.as_raw_fd()]);
    manager.send_send_message_array(&[b"Hello", b"via", b"array!"]);
    manager.send_send_message_array(&[]);
    manager.send_send_message_array_uint(&[69, 420, 1337]);

    socket.roundtrip().unwrap();

    let obj = manager.send_make_object().unwrap();
    let obj = test_protocol_v1::client::MyObjectV1Object::new(obj, &mut state);

    obj.send_send_message(b"Hello on object");
    obj.send_send_enum(test_protocol_v1::spec::MyEnum::World);

    while socket.dispatch_events(true).is_ok() {}
}
