use hyprwire::implementation::client::ProtocolImplementations;
use hyprwire::implementation::types::ProtocolSpec;
use hyprwire::{client, server};
use nix::{libc, poll};
use std::io::{Read, Write};
use std::os::fd;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net;
use std::{fs, io, path, process};

const TEST_PROTOCOL_VERSION: u32 = 1;

mod server_socket {
    mod test_protocol_v1 {
        hyprwire::include_protocol!("test_protocol_v1");
        pub use server::*;
        pub use spec::*;
    }

    use super::*;

    #[derive(Default)]
    struct App {
        quit: bool,
        manager: Option<test_protocol_v1::MyManagerV1Object>,
        objects: Vec<test_protocol_v1::MyObjectV1Object>,
    }

    impl hyprwire::Dispatch<test_protocol_v1::MyManagerV1Object> for App {
        fn event(
            &mut self,
            object: &test_protocol_v1::MyManagerV1Object,
            event: <test_protocol_v1::MyManagerV1Object as hyprwire::Object>::Event<'_>,
        ) {
            match event {
                test_protocol_v1::MyManagerV1Event::SendMessage { message } => {
                    println!("Recvd message: {}", message);
                }
                test_protocol_v1::MyManagerV1Event::SendMessageArrayFd { message } => {
                    println!("Received {} fds", message.len());

                    for fd in message {
                        let raw_fd = fd.as_raw_fd();
                        let mut file = fs::File::from(fd);
                        let mut buf = [0u8; 64];
                        let n = file.read(&mut buf).unwrap_or(0);
                        let data = String::from_utf8_lossy(&buf[..n]);
                        println!("fd {} with data: {}", raw_fd, data);
                    }
                }
                test_protocol_v1::MyManagerV1Event::SendMessageFd { message } => {
                    let raw_fd = message.as_raw_fd();
                    let mut file = fs::File::from(message);
                    let mut buf = [0u8; 64];
                    let n = file.read(&mut buf).unwrap_or(0);
                    let data = String::from_utf8_lossy(&buf[..n]);
                    println!("Recvd fd {} with data: {}", raw_fd, data);
                }
                test_protocol_v1::MyManagerV1Event::SendMessageArray { message } => {
                    let data: Vec<&str> = message.iter().map(|s| s.as_str()).collect();
                    println!("Got array message: \"{}\"", data.join(", "));
                }
                test_protocol_v1::MyManagerV1Event::SendMessageArrayUint { message } => {
                    let data: Vec<String> = message.iter().map(u32::to_string).collect();
                    println!("Got uint array message: \"{}\"", data.join(", "));
                }
                test_protocol_v1::MyManagerV1Event::MakeObject { seq } => {
                    let object = object
                        .make_object::<Self>(seq)
                        .expect("failed to create object");
                    object.send_send_message("Hello object");
                    self.objects.push(object);
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

                    self.quit = true;
                    object.error(
                        test_protocol_v1::MyErrorEnum::ErrorImportant as u32,
                        "Important error occurred!",
                    );
                }
                test_protocol_v1::MyObjectV1Event::MakeObject { seq } => {
                    let object = object
                        .make_object::<Self>(seq)
                        .expect("failed to create nested object");
                    object.send_send_message("Hello object");
                    self.objects.push(object);
                }
                test_protocol_v1::MyObjectV1Event::Destroy => {}
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

    pub fn main(client_fd: net::UnixStream) -> io::Result<()> {
        let mut socket = server::Server::open::<&path::Path>(None)?;
        let mut app = App::default();
        let implementation = test_protocol_v1::TestProtocolV1Impl::new(1, &mut app);
        socket.add_implementation(implementation);
        if !poll_readable(client_fd.as_fd(), 1000)? {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "failed to wait for client hello",
            ));
        }

        socket.add_client(client_fd);

        while !app.quit {
            if !poll_readable(socket.extract_loop_fd()?, -1)? {
                continue;
            }

            if !socket.dispatch_events(&mut app, false) {
                break;
            }
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

    use super::*;

    #[derive(Default)]
    struct App {
        quit: bool,
        object: Option<test_protocol_v1::MyObjectV1Object>,
        object2: Option<test_protocol_v1::MyObjectV1Object>,
    }

    impl hyprwire::Dispatch<test_protocol_v1::MyManagerV1Object> for App {
        fn event(
            &mut self,
            object: &test_protocol_v1::MyManagerV1Object,
            event: <test_protocol_v1::MyManagerV1Object as hyprwire::Object>::Event<'_>,
        ) {
            let _ = object;
            match event {
                test_protocol_v1::MyManagerV1Event::SendMessage { message } => {
                    println!("Server says {}", message);
                }
                test_protocol_v1::MyManagerV1Event::RecvMessageArrayUint { message } => {
                    println!("Server sent uint array {:?}", message);
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
            let test_protocol_v1::MyObjectV1Event::SendMessage { message } = event;
            println!("Server says on object {}", message);

            if self.object2.as_ref() == Some(object) {
                if let Some(object) = self.object.as_ref() {
                    object.send_send_enum(test_protocol_v1::MyEnum::World);
                }
                self.quit = true;
            }
        }
    }

    pub fn main(server_fd: net::UnixStream) -> io::Result<()> {
        let mut socket = client::Client::from_fd(server_fd)?;
        let mut app = App::default();
        let implementation = test_protocol_v1::TestProtocolV1Impl::default();
        socket.add_implementation(implementation.clone());
        socket.wait_for_handshake(&mut app)?;

        println!("OK!");

        let spec = socket
            .get_spec(implementation.protocol().spec_name())
            .ok_or_else(|| io::Error::other("test protocol unsupported"))?;

        println!(
            "test protocol supported at version {}. Binding.",
            spec.spec_ver()
        );

        let manager = socket
            .bind::<test_protocol_v1::MyManagerV1Object, App>(
                implementation.protocol(),
                TEST_PROTOCOL_VERSION,
                &mut app,
            )
            .map_err(io::Error::other)?;

        println!("Bound!");

        let mut pipes = net::UnixStream::pair().unwrap();
        let buf = b"pipe!";
        pipes.1.write_all(buf).unwrap();
        drop(pipes.1);

        println!("Will send fd {}\n", pipes.0.as_raw_fd());

        manager.send_send_message("Hello!");
        manager.send_send_message_fd(&pipes.0);
        manager.send_send_message_array(&["Hello", "via", "array!"]);
        manager.send_send_message_array::<&str>(&[]);
        manager.send_send_message_array_uint(&[69, 420, 2137]);

        let object = manager
            .send_make_object::<App>()
            .ok_or_else(|| io::Error::other("failed to create first object"))?;
        let object2 = object
            .send_make_object::<App>()
            .ok_or_else(|| io::Error::other("failed to create second object"))?;

        app.object = Some(object.clone());
        app.object2 = Some(object2.clone());

        object.send_send_message("Hello from object");
        object2.send_send_message("Hello from object2");

        while !app.quit {
            socket.dispatch_events(&mut app, true)?;
        }

        let _ = socket.roundtrip(&mut app);

        Ok(())
    }
}

fn poll_readable(fd: fd::BorrowedFd, timeout_ms: i32) -> io::Result<bool> {
    let mut pfd = [poll::PollFd::new(fd, poll::PollFlags::POLLIN)];

    loop {
        let rc = poll::poll(&mut pfd, poll::PollTimeout::try_from(timeout_ms).unwrap())?;
        if rc >= 0 {
            return Ok(rc > 0
                && pfd[0]
                    .revents()
                    .is_some_and(|revents| revents.contains(poll::PollFlags::POLLIN)));
        }
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
