mod test_protocol_v1 {
    hyprwire::include_protocol!("test_protocol_v1");
    pub use server::*;
    pub use spec::*;
}

use calloop::generic;
use hyprwire::server;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::str::FromStr;
use std::{env, fs, path};
use test_protocol_v1::{my_manager_v1, my_object_v1};

fn socket_path() -> path::PathBuf {
    let mut runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap();
    runtime_dir.push_str("/test-hw.sock");

    path::PathBuf::from_str(&runtime_dir).unwrap()
}

#[derive(Default)]
struct App {
    manager: Option<my_manager_v1::MyManagerV1>,
    object: Option<my_object_v1::MyObjectV1>,
}

impl hyprwire::Dispatch<my_manager_v1::MyManagerV1> for App {
    fn event(
        &mut self,
        object: &my_manager_v1::MyManagerV1,
        event: <my_manager_v1::MyManagerV1 as hyprwire::Object>::Event<'_>,
    ) {
        match event {
            my_manager_v1::Event::SendMessage { message } => {
                println!("Recvd message: {}", message)
            }
            my_manager_v1::Event::SendMessageArrayFd { message } => {
                println!("Received {} fds", message.len());

                for fd in message {
                    let mut file = fs::File::from(fd);
                    let mut buf = [0u8; 64];
                    let n = file.read(&mut buf).unwrap_or(0);
                    let data = String::from_utf8_lossy(&buf[..n]);
                    println!("fd {} with data: {}", file.as_raw_fd(), data);
                }
            }
            my_manager_v1::Event::SendMessageFd { message } => {
                let mut file = fs::File::from(message);
                let mut buf = [0u8; 64];
                let n = file.read(&mut buf).unwrap_or(0);
                let data = String::from_utf8_lossy(&buf[..n]);
                println!("Recvd fd {} with data: {}", file.as_raw_fd(), data);
            }
            my_manager_v1::Event::SendMessageArray { message } => {
                println!("Got array message: \"{}\"", message.join(", "));
            }
            my_manager_v1::Event::SendMessageArrayUint { message } => {
                let conct: Vec<String> = message.iter().map(|v| v.to_string()).collect();
                println!("Got uint array message: \"{}\"", conct.join(", "));
            }
            my_manager_v1::Event::MakeObject { seq } => {
                let obj = object
                    .make_object::<Self>(seq)
                    .expect("failed to create object");
                obj.send_send_message("Hello object");
                self.object = Some(obj);
            }
        }
    }
}

impl hyprwire::Dispatch<my_object_v1::MyObjectV1> for App {
    fn event(
        &mut self,
        object: &my_object_v1::MyObjectV1,
        event: <my_object_v1::MyObjectV1 as hyprwire::Object>::Event<'_>,
    ) {
        match event {
            my_object_v1::Event::SendMessage { message } => {
                println!("Object says hello: {}", message);
            }
            my_object_v1::Event::SendEnum { message } => {
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
    fn bind(&mut self, object: my_manager_v1::MyManagerV1) {
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
    let mut socket = server::Server::bind(&path).unwrap();
    let mut state = App::default();
    socket.add_implementation::<test_protocol_v1::TestProtocolV1Impl, _>(1, &mut state);

    let mut event_loop = calloop::EventLoop::try_new().unwrap();

    let fd_wrapper = unsafe { generic::FdWrapper::new(socket.extract_loop_fd().as_raw_fd()) };
    let source = generic::Generic::new(
        fd_wrapper,
        calloop::Interest {
            readable: true,
            writable: false,
        },
        calloop::Mode::Level,
    );
    event_loop
        .handle()
        .insert_source(source, move |_, _, state| {
            _ = socket.dispatch_events(state, false);
            Ok(calloop::PostAction::Continue)
        })
        .unwrap();

    event_loop.run(None, &mut state, |_| {}).unwrap();
}
