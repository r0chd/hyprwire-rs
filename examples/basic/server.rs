mod test_protocol_v1 {
    include!(concat!(env!("OUT_DIR"), "/test_protocol_v1.rs"));
}

use hyprwire::server;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::str::FromStr;
use std::{env, fs, path};

fn socket_path() -> path::PathBuf {
    let mut runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap();
    runtime_dir.push_str("/test-hw.sock");

    path::PathBuf::from_str(&runtime_dir).unwrap()
}

#[derive(Default)]
struct App {
    manager: Option<test_protocol_v1::server::MyManagerV1Object>,
    object: Option<test_protocol_v1::server::MyObjectV1Object>,
}

impl hyprwire::Dispatch<test_protocol_v1::server::MyManagerV1Object> for App {
    fn event(
        &mut self,
        proxy: &test_protocol_v1::server::MyManagerV1Object,
        event: test_protocol_v1::server::MyManagerV1Event<'_>,
    ) {
        match event {
            test_protocol_v1::server::MyManagerV1Event::SendMessage { message } => {
                println!("Recvd message: {}", message.to_string_lossy())
            }
            test_protocol_v1::server::MyManagerV1Event::SendMessageArrayFd { message } => {
                println!("Received {} fds", message.len());

                for fd in message {
                    let mut file = unsafe { fs::File::from_raw_fd(*fd) };
                    let mut buf = [0u8; 64];
                    let n = file.read(&mut buf).unwrap_or(0);
                    let data = String::from_utf8_lossy(&buf[..n]);
                    println!("fd {} with data: {}", fd, data);
                }
            }
            test_protocol_v1::server::MyManagerV1Event::SendMessageFd { message } => {
                let mut file = unsafe { fs::File::from_raw_fd(message) };
                let mut buf = [0u8; 64];
                let n = file.read(&mut buf).unwrap_or(0);
                let data = String::from_utf8_lossy(&buf[..n]);
                println!("Recvd fd {} with data: {}", message, data);
            }
            test_protocol_v1::server::MyManagerV1Event::SendMessageArray { message } => {
                let conct: Vec<&str> = message.iter().map(|s| s.to_str().unwrap_or("")).collect();
                println!("Got array message: \"{}\"", conct.join(", "));
            }
            test_protocol_v1::server::MyManagerV1Event::SendMessageArrayUint { message } => {
                let conct: Vec<String> = message.iter().map(|v| v.to_string()).collect();
                println!("Got uint array message: \"{}\"", conct.join(", "));
            }
            test_protocol_v1::server::MyManagerV1Event::MakeObject { seq } => {
                let obj = proxy
                    .create_object::<test_protocol_v1::server::MyObjectV1Object, Self>(seq)
                    .expect("failed to create object");
                obj.send_send_message("Hello object");
                self.object = Some(obj);
            }
        }
    }
}

impl hyprwire::Dispatch<test_protocol_v1::server::MyObjectV1Object> for App {
    fn event(
        &mut self,
        proxy: &test_protocol_v1::server::MyObjectV1Object,
        event: <test_protocol_v1::server::MyObjectV1Object as hyprwire::Proxy>::Event<'_>,
    ) {
        match event {
            test_protocol_v1::server::MyObjectV1Event::SendMessage { message } => {
                println!("Object says hello: {}", message.to_string_lossy());
            }
            test_protocol_v1::server::MyObjectV1Event::SendEnum { message } => {
                println!("Object sent enum: {:?}", message);

                println!("Erroring out the client!");

                proxy.error(
                    test_protocol_v1::spec::MyErrorEnum::ErrorImportant as u32,
                    "Important error occurred!",
                );
            }
            _ => {}
        }
    }
}

impl test_protocol_v1::server::TestProtocolV1Handler for App {
    fn bind(&mut self, object: hyprwire::implementation::types::Object) {
        println!("Object bound XD");
        let manager = test_protocol_v1::server::MyManagerV1Object::new::<Self>(object);
        manager.send_send_message("Hello manager");
        self.manager = Some(manager);
    }
}

fn main() {
    env_logger::Builder::new()
        .filter(None, log::LevelFilter::Trace)
        .init();

    let path = socket_path();
    let mut sock = server::Server::open(Some(&path)).unwrap();
    let mut app = App::default();
    let implementation = test_protocol_v1::server::TestProtocolV1Impl::new(1, &mut app);
    sock.add_implementation(implementation);

    while sock.dispatch_events(&mut app, true) {}
}
