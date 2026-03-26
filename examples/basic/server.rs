mod test_protocol_v1 {
    hyprwire::include_protocol!("test_protocol_v1");
    pub use server::*;
}

use hyprwire::server;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::str::FromStr;
use std::{env, fs, path};

fn socket_path() -> path::PathBuf {
    let mut runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap();
    runtime_dir.push_str("/test-hw.sock");

    path::PathBuf::from_str(&runtime_dir).unwrap()
}

#[derive(Default)]
struct App {
    manager: Option<test_protocol_v1::MyManagerV1Object>,
    object: Option<test_protocol_v1::MyObjectV1Object>,
}

impl hyprwire::Dispatch<test_protocol_v1::MyManagerV1Object> for App {
    fn event(
        &mut self,
        object: &test_protocol_v1::MyManagerV1Object,
        event: test_protocol_v1::MyManagerV1Event,
    ) {
        match event {
            test_protocol_v1::MyManagerV1Event::SendMessage { message } => {
                println!("Recvd message: {}", message)
            }
            test_protocol_v1::MyManagerV1Event::SendMessageArrayFd { message } => {
                println!("Received {} fds", message.len());

                for fd in message {
                    let mut file = fs::File::from(fd);
                    let mut buf = [0u8; 64];
                    let n = file.read(&mut buf).unwrap_or(0);
                    let data = String::from_utf8_lossy(&buf[..n]);
                    println!("fd {} with data: {}", file.as_raw_fd(), data);
                }
            }
            test_protocol_v1::MyManagerV1Event::SendMessageFd { message } => {
                let mut file = fs::File::from(message);
                let mut buf = [0u8; 64];
                let n = file.read(&mut buf).unwrap_or(0);
                let data = String::from_utf8_lossy(&buf[..n]);
                println!("Recvd fd {} with data: {}", file.as_raw_fd(), data);
            }
            test_protocol_v1::MyManagerV1Event::SendMessageArray { message } => {
                println!("Got array message: \"{}\"", message.join(", "));
            }
            test_protocol_v1::MyManagerV1Event::SendMessageArrayUint { message } => {
                let conct: Vec<String> = message.iter().map(|v| v.to_string()).collect();
                println!("Got uint array message: \"{}\"", conct.join(", "));
            }
            test_protocol_v1::MyManagerV1Event::MakeObject { seq } => {
                let obj = object
                    .create_make_object::<Self>(seq)
                    .expect("failed to create object");
                obj.send_send_message("Hello object");
                self.object = Some(obj);
            }
        }
    }
}

impl hyprwire::Dispatch<test_protocol_v1::MyObjectV1Object> for App {
    fn event(
        &mut self,
        object: &test_protocol_v1::MyObjectV1Object,
        event: <test_protocol_v1::MyObjectV1Object as hyprwire::Object>::Event<'_>,
    ) {
        match event {
            test_protocol_v1::MyObjectV1Event::SendMessage { message } => {
                println!("Object says hello: {}", message);
            }
            test_protocol_v1::MyObjectV1Event::SendEnum { message } => {
                println!("Object sent enum: {:?}", message);

                println!("Erroring out the client!");

                object.error(
                    test_protocol_v1::MyErrorEnum::ErrorImportant as u32,
                    "Important error occurred!",
                );
            }
            _ => {}
        }
    }
}

impl test_protocol_v1::TestProtocolV1Handler for App {
    fn bind(&mut self, object: test_protocol_v1::MyManagerV1Object) {
        println!("Object bound XD");
        object.send_send_message("Hello manager");
        self.manager = Some(object);
    }
}

fn main() {
    env_logger::Builder::new()
        .filter(None, log::LevelFilter::Trace)
        .init();

    let path = socket_path();
    let mut sock = server::Server::open(Some(&path)).unwrap();
    let mut app = App::default();
    let implementation = test_protocol_v1::TestProtocolV1Impl::new(1, &mut app);
    sock.add_implementation(implementation);

    while sock.dispatch_events(&mut app, true) {}
}
