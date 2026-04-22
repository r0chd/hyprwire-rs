use nix::libc;
use std::os::unix::net;
use std::process;

const TEST_PROTOCOL_VERSION: u32 = 1;

mod server_socket {
    mod test_protocol_v1 {
        hyprwire::include_protocol!("test_protocol_v1");
        pub use server::*;
        pub use spec::*;
    }

    use super::TEST_PROTOCOL_VERSION;
    use hyprwire::server;
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::net;
    use std::{fs, path};
    use test_protocol_v1::{my_manager_v1, my_object_v1};

    #[derive(Default)]
    struct App {
        quit: bool,
        manager: Option<my_manager_v1::MyManagerV1>,
        objects: Vec<my_object_v1::MyObjectV1>,
    }

    impl hyprwire::Dispatch<my_manager_v1::MyManagerV1> for App {
        fn event(
            &mut self,
            object: &my_manager_v1::MyManagerV1,
            event: <my_manager_v1::MyManagerV1 as hyprwire::Object>::Event<'_>,
        ) {
            match event {
                my_manager_v1::Event::SendMessage { message } => {
                    println!("Recvd message: {}", message);
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
                    let object = object
                        .make_object::<Self>(seq)
                        .expect("failed to create object");
                    object.send_send_message("Hello object");
                    self.objects.push(object);
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

                    self.quit = true;
                    object.error(
                        test_protocol_v1::MyErrorEnum::ErrorImportant as u32,
                        "Important error occurred!",
                    );
                }
                my_object_v1::Event::MakeObject { seq } => {
                    let object = object
                        .make_object::<Self>(seq)
                        .expect("failed to create nested object");
                    object.send_send_message("Hello object");
                    self.objects.push(object);
                }
                my_object_v1::Event::Destroy => {}
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

    pub fn main(client_fd: net::UnixStream) -> hyprwire::Result<()> {
        let mut socket =
            server::Server::open::<path::PathBuf>(None).map_err(hyprwire::Error::Io)?;
        let mut app = App::default();
        socket.add_implementation::<test_protocol_v1::TestProtocolV1Impl, _>(
            TEST_PROTOCOL_VERSION,
            &mut app,
        );

        socket.add_client(client_fd)?;

        while !app.quit {
            socket.dispatch_events(&mut app, true)?;
        }

        Ok(())
    }
}

mod client_socket {
    mod test_protocol_v1 {
        hyprwire::include_protocol!("test_protocol_v1");
        pub use client::*;
        pub use spec::*;
    }

    use hyprwire::client;
    use hyprwire_core::types::ProtocolSpec;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net;
    use test_protocol_v1::{my_manager_v1, my_object_v1};

    #[derive(Default)]
    struct App {
        quit: bool,
        object: Option<my_object_v1::MyObjectV1>,
        object2: Option<my_object_v1::MyObjectV1>,
    }

    impl hyprwire::Dispatch<my_manager_v1::MyManagerV1> for App {
        fn event(
            &mut self,
            _object: &my_manager_v1::MyManagerV1,
            event: <my_manager_v1::MyManagerV1 as hyprwire::Object>::Event<'_>,
        ) {
            match event {
                my_manager_v1::Event::SendMessage { message } => {
                    println!("Server says {}", message);
                }
                my_manager_v1::Event::RecvMessageArrayUint { message } => {
                    println!("Server sent uint array {:?}", message);
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
            let my_object_v1::Event::SendMessage { message } = event;
            println!("Server says on object {}", message);

            if self.object2.as_ref() == Some(object) {
                if let Some(object) = self.object.as_ref() {
                    object.send_send_enum(test_protocol_v1::MyEnum::World);
                }
                self.quit = true;
            }
        }
    }

    pub fn main(server_fd: net::UnixStream) -> hyprwire::Result<()> {
        let mut socket = client::Client::from_fd(server_fd).map_err(hyprwire::Error::Io)?;
        let mut app = App::default();
        socket.add_implementation::<test_protocol_v1::TestProtocolV1Impl>();
        socket.wait_for_handshake(&mut app)?;

        println!("OK!");

        let spec = socket
            .get_spec::<test_protocol_v1::TestProtocolV1Impl>()
            .ok_or(hyprwire::Error::ProtocolViolation(
                hyprwire::core::message::Error::NoSpec,
            ))?;

        println!(
            "test protocol supported at version {}. Binding.",
            spec.spec_ver()
        );

        let manager =
            socket.bind::<my_manager_v1::MyManagerV1, App>(&spec, spec.spec_ver(), &mut app)?;

        println!("Bound!");

        let mut pipes = net::UnixStream::pair().unwrap();
        pipes.1.write_all(b"pipe!").unwrap();
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
        manager.send_send_message_array_uint(&[69, 420, 2137]);

        let object = manager
            .send_make_object::<App>()
            .ok_or(hyprwire::Error::ConnectionClosed)?;
        let object2 = object
            .send_make_object::<App>()
            .ok_or(hyprwire::Error::ConnectionClosed)?;

        app.object = Some(object.clone());
        app.object2 = Some(object2.clone());

        object.send_send_message("Hello from object");
        object2.send_send_message("Hello from object2");

        while !app.quit {
            if let Err(err) = socket.dispatch_events(&mut app, true) {
                eprintln!("client dispatch error: {err}");
                break;
            }
        }

        let _ = socket.roundtrip(&mut app);

        Ok(())
    }
}

fn main() {
    env_logger::Builder::new()
        .filter(None, log::LevelFilter::Trace)
        .init();

    let (server_fd, client_fd) = net::UnixStream::pair().expect("failed to create socketpair");

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        drop(server_fd);
        drop(client_fd);
        panic!("failed to fork");
    }

    if pid == 0 {
        drop(server_fd);

        if let Err(err) = client_socket::main(client_fd) {
            eprintln!("client error: {err}");
            process::exit(1);
        }

        process::exit(0);
    }

    drop(client_fd);

    if let Err(err) = server_socket::main(server_fd) {
        eprintln!("server error: {err}");
    }

    unsafe {
        let mut status = 0;
        libc::waitpid(pid, &mut status, 0);
    }
}
