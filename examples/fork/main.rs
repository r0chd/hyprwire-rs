mod test_protocol_v1 {
    hyprwire::include_protocol!("test_protocol_v1");
    pub use spec::*;
}

use hyprwire::client;
use hyprwire::implementation::client::ProtocolImplementations;
use hyprwire::implementation::types::ProtocolSpec;
use hyprwire::server;
use nix::libc;
use std::io;
use std::io::Read;
use std::os::fd;
use std::os::fd::{AsRawFd, FromRawFd};
use std::{fs, path};

const TEST_PROTOCOL_VERSION: u32 = 1;

#[derive(Default)]
struct ServerApp {
    quit: bool,
    manager: Option<test_protocol_v1::server::MyManagerV1Object>,
    objects: Vec<test_protocol_v1::server::MyObjectV1Object>,
}

impl hyprwire::Dispatch<test_protocol_v1::server::MyManagerV1Object> for ServerApp {
    fn event(
        &mut self,
        object: &test_protocol_v1::server::MyManagerV1Object,
        event: <test_protocol_v1::server::MyManagerV1Object as hyprwire::Object>::Event<'_>,
    ) {
        match event {
            test_protocol_v1::server::MyManagerV1Event::SendMessage { message } => {
                println!("Recvd message: {}", message);
            }
            test_protocol_v1::server::MyManagerV1Event::SendMessageArrayFd { message } => {
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
            test_protocol_v1::server::MyManagerV1Event::SendMessageFd { message } => {
                let raw_fd = message.as_raw_fd();
                let mut file = fs::File::from(message);
                let mut buf = [0u8; 64];
                let n = file.read(&mut buf).unwrap_or(0);
                let data = String::from_utf8_lossy(&buf[..n]);
                println!("Recvd fd {} with data: {}", raw_fd, data);
            }
            test_protocol_v1::server::MyManagerV1Event::SendMessageArray { message } => {
                let data: Vec<&str> = message.iter().map(|s| s.as_str()).collect();
                println!("Got array message: \"{}\"", data.join(", "));
            }
            test_protocol_v1::server::MyManagerV1Event::SendMessageArrayUint { message } => {
                let data: Vec<String> = message.iter().map(u32::to_string).collect();
                println!("Got uint array message: \"{}\"", data.join(", "));
            }
            test_protocol_v1::server::MyManagerV1Event::MakeObject { seq } => {
                let object = object
                    .make_object::<Self>(seq)
                    .expect("failed to create object");
                object.send_send_message("Hello object");
                self.objects.push(object);
            }
        }
    }
}

impl hyprwire::Dispatch<test_protocol_v1::server::MyObjectV1Object> for ServerApp {
    fn event(
        &mut self,
        object: &test_protocol_v1::server::MyObjectV1Object,
        event: <test_protocol_v1::server::MyObjectV1Object as hyprwire::Object>::Event<'_>,
    ) {
        match event {
            test_protocol_v1::server::MyObjectV1Event::SendMessage { message } => {
                println!("Object says hello: {}", message);
            }
            test_protocol_v1::server::MyObjectV1Event::SendEnum { message } => {
                println!("Object sent enum: {:?}", message);
                println!("Erroring out the client!");

                self.quit = true;
                object.error(
                    test_protocol_v1::MyErrorEnum::ErrorImportant as u32,
                    "Important error occurred!",
                );
            }
            test_protocol_v1::server::MyObjectV1Event::MakeObject { seq } => {
                let object = object
                    .make_object::<Self>(seq)
                    .expect("failed to create nested object");
                object.send_send_message("Hello object");
                self.objects.push(object);
            }
            test_protocol_v1::server::MyObjectV1Event::Destroy => {}
        }
    }
}

impl test_protocol_v1::server::TestProtocolV1Handler for ServerApp {
    fn bind(&mut self, object: test_protocol_v1::server::MyManagerV1Object) {
        println!("Object bound XD");
        object.send_send_message("Hello manager");
        self.manager = Some(object);
    }
}

#[derive(Default)]
struct ClientApp {
    quit: bool,
    object: Option<test_protocol_v1::client::MyObjectV1Object>,
    object2: Option<test_protocol_v1::client::MyObjectV1Object>,
}

impl hyprwire::Dispatch<test_protocol_v1::client::MyManagerV1Object> for ClientApp {
    fn event(
        &mut self,
        object: &test_protocol_v1::client::MyManagerV1Object,
        event: <test_protocol_v1::client::MyManagerV1Object as hyprwire::Object>::Event<'_>,
    ) {
        let _ = object;
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

impl hyprwire::Dispatch<test_protocol_v1::client::MyObjectV1Object> for ClientApp {
    fn event(
        &mut self,
        object: &test_protocol_v1::client::MyObjectV1Object,
        event: <test_protocol_v1::client::MyObjectV1Object as hyprwire::Object>::Event<'_>,
    ) {
        let test_protocol_v1::client::MyObjectV1Event::SendMessage { message } = event;
        println!("Server says on object {}", message);

        if self.object2.as_ref() == Some(object) {
            if let Some(object) = self.object.as_ref() {
                object.send_send_enum(test_protocol_v1::MyEnum::World);
            }
            self.quit = true;
        }
    }
}

fn poll_readable(fd: fd::RawFd, timeout_ms: i32) -> io::Result<bool> {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    loop {
        let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if rc >= 0 {
            return Ok(rc > 0 && (pfd.revents & libc::POLLIN) != 0);
        }

        let err = io::Error::last_os_error();
        if matches!(err.raw_os_error(), Some(libc::EINTR | libc::EAGAIN)) {
            continue;
        }
        return Err(err);
    }
}

fn make_pipe_with_message(message: &[u8]) -> io::Result<fd::OwnedFd> {
    let mut pipe_fds = [0; 2];
    let rc = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }

    let wrote = unsafe { libc::write(pipe_fds[1], message.as_ptr().cast(), message.len()) };
    if wrote < 0 {
        let err = io::Error::last_os_error();
        unsafe {
            libc::close(pipe_fds[0]);
            libc::close(pipe_fds[1]);
        }
        return Err(err);
    }

    let [read_fd, write_fd] = pipe_fds;
    let read_fd = unsafe { fd::OwnedFd::from_raw_fd(read_fd) };
    let write_fd = unsafe { fd::OwnedFd::from_raw_fd(write_fd) };
    drop(write_fd);

    Ok(read_fd)
}

fn server_main(client_fd: fd::RawFd) -> io::Result<()> {
    let mut socket = server::Server::open::<&path::Path>(None)?;
    let mut app = ServerApp::default();
    let implementation = test_protocol_v1::server::TestProtocolV1Impl::new(1, &mut app);
    socket.add_implementation(implementation);
    if !poll_readable(client_fd, 1000)? {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "failed to wait for client hello",
        ));
    }

    socket.add_client(unsafe { fd::OwnedFd::from_raw_fd(client_fd) });

    while !app.quit {
        if !poll_readable(socket.extract_loop_fd()?.as_raw_fd(), -1)? {
            continue;
        }

        if !socket.dispatch_events(&mut app, false) {
            break;
        }
    }

    Ok(())
}

fn client_main(server_fd: fd::OwnedFd) -> io::Result<()> {
    let mut socket = client::Client::from_fd(server_fd);
    let implementation = test_protocol_v1::client::TestProtocolV1Impl::default();
    socket.add_implementation(implementation.clone());
    socket.wait_for_handshake()?;

    println!("OK!");

    let spec = socket
        .get_spec(implementation.protocol().spec_name())
        .ok_or_else(|| io::Error::other("test protocol unsupported"))?;

    println!(
        "test protocol supported at version {}. Binding.",
        spec.spec_ver()
    );

    let mut app = ClientApp::default();
    let manager = socket
        .bind::<test_protocol_v1::client::MyManagerV1Object, ClientApp>(
            implementation.protocol(),
            TEST_PROTOCOL_VERSION,
        )
        .map_err(io::Error::other)?;

    println!("Bound!");

    let pipe_fd = make_pipe_with_message(b"pipe!")?;
    println!("Will send fd {}", pipe_fd.as_raw_fd());

    manager.send_send_message("Hello!");
    manager.send_send_message_fd(&pipe_fd);
    manager.send_send_message_array(&["Hello", "via", "array!"]);
    manager.send_send_message_array::<&str>(&[]);
    manager.send_send_message_array_uint(&[69, 420, 2137]);

    let object = manager
        .send_make_object::<ClientApp>()
        .ok_or_else(|| io::Error::other("failed to create first object"))?;
    let object2 = object
        .send_make_object::<ClientApp>()
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

fn socketpair() -> io::Result<[fd::RawFd; 2]> {
    let mut fds = [0; 2];
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(fds)
}

fn main() {
    env_logger::Builder::new()
        .filter(None, log::LevelFilter::Trace)
        .init();

    let [server_fd, client_fd] = socketpair().expect("failed to create socketpair");

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        unsafe {
            libc::close(server_fd);
            libc::close(client_fd);
        }
        panic!("failed to fork");
    }

    if pid == 0 {
        unsafe {
            libc::close(server_fd);
        }

        if let Err(err) = client_main(unsafe { fd::OwnedFd::from_raw_fd(client_fd) }) {
            eprintln!("client error: {err}");
            unsafe { libc::_exit(1) };
        }

        unsafe { libc::_exit(0) };
    }

    unsafe {
        libc::close(client_fd);
    }

    if let Err(err) = server_main(server_fd) {
        eprintln!("server error: {err}");
    }

    unsafe {
        let mut status = 0;
        libc::waitpid(pid, &mut status, 0);
    }
}
