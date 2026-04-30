use hyprwire::{client, server};
use nix::{libc, poll};
use std::io;
use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::net;
use std::process;

const INTEGRATION_TESTS_PROTOCOL_VERSION: u32 = 1;

mod server_main {
    pub mod integration_tests_v1 {
        hyprwire::include_protocol!("integration_test_protocol_v1");
        pub use server::*;
    }
    use integration_tests_v1::integration_manager_v1;

    pub struct ServerApp {
        pub message: Option<String>,
    }

    impl hyprwire::Dispatch<integration_manager_v1::IntegrationManagerV1> for ServerApp {
        fn event(
            &mut self,
            _object: &integration_manager_v1::IntegrationManagerV1,
            event: <integration_manager_v1::IntegrationManagerV1 as hyprwire::Object>::Event<'_>,
        ) {
            match event {
                integration_manager_v1::Event::SendMessage { message } => {
                    self.message = Some(message);
                }
                integration_manager_v1::Event::SendUint { value } => {
                    _ = value;
                }
                integration_manager_v1::Event::SendInt { value } => {
                    _ = value;
                }
                integration_manager_v1::Event::SendFloat { value } => {
                    _ = value;
                }
                integration_manager_v1::Event::SendFd { value } => {
                    _ = value;
                }
                integration_manager_v1::Event::SendArrayUint { values } => {
                    _ = values;
                }
                integration_manager_v1::Event::SendArrayString { values } => {
                    _ = values;
                }
                integration_manager_v1::Event::SendArrayFd { values } => {
                    _ = values;
                }
                integration_manager_v1::Event::SendStart { cmd, env } => {
                    _ = cmd;
                    _ = env;
                }
                integration_manager_v1::Event::SendMixed { a, b, c, d } => {
                    _ = a;
                    _ = b;
                    _ = c;
                    _ = d;
                }
                integration_manager_v1::Event::SendEnum { value } => {
                    _ = value;
                }
                integration_manager_v1::Event::MakeObject { seq } => {
                    _ = seq;
                }
            }
        }
    }

    impl integration_tests_v1::IntegrationTestProtocolV1Handler for ServerApp {
        fn bind(&mut self, _object: integration_manager_v1::IntegrationManagerV1) {}
    }
}

mod client_main {
    mod integration_tests_v1 {
        hyprwire::include_protocol!("integration_test_protocol_v1");
        pub use client::*;
    }
    use super::*;
    use integration_tests_v1::integration_manager_v1;

    struct ClientApp;

    impl hyprwire::Dispatch<integration_manager_v1::IntegrationManagerV1> for ClientApp {
        fn event(
            &mut self,
            _object: &integration_manager_v1::IntegrationManagerV1,
            event: <integration_manager_v1::IntegrationManagerV1 as hyprwire::Object>::Event<'_>,
        ) {
            match event {
                integration_tests_v1::client::integration_manager_v1::Event::RecvArrayUint {
                    values,
                } => {
                    _ = values;
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvMessage {
                    message,
                } => {
                    _ = message;
                }
                integration_tests_v1::client::integration_manager_v1::Event::ReportError {
                    code,
                    message,
                } => {
                    _ = code;
                    _ = message;
                }
            }
        }
    }

    pub fn main(
        server_stream: net::UnixStream,
        mut shutdown_write: net::UnixStream,
    ) -> hyprwire::Result<()> {
        let mut socket = client::Client::from_fd(server_stream).map_err(hyprwire::Error::Io)?;
        let mut app = ClientApp;

        socket.add_implementation::<integration_tests_v1::IntegrationTestProtocolV1Impl>();
        socket.wait_for_handshake(&mut app)?;

        let spec = socket
            .get_spec::<integration_tests_v1::IntegrationTestProtocolV1Impl>()
            .ok_or(hyprwire::Error::ProtocolViolation(
                hyprwire::core::message::Error::NoSpec,
            ))?;

        let manager = socket.bind::<integration_manager_v1::IntegrationManagerV1, ClientApp>(
            &spec,
            INTEGRATION_TESTS_PROTOCOL_VERSION,
            &mut app,
        )?;

        manager.send_send_message("Hello!");
        socket.roundtrip(&mut app).unwrap();

        let _ = shutdown_write.write_all(b"x");
        Ok(())
    }
}

#[test]
fn integration_protocol_roundtrip() -> io::Result<()> {
    let (server_stream, client_stream) = net::UnixStream::pair()?;
    let (shutdown_read, shutdown_write) = net::UnixStream::pair()?;

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::other("fork failed"));
    }

    if pid == 0 {
        drop(server_stream);
        drop(shutdown_read);

        if let Err(err) = client_main::main(client_stream, shutdown_write) {
            eprintln!("client error: {err}");
            process::exit(1);
        }

        process::exit(0);
    }

    drop(client_stream);
    drop(shutdown_write);

    let mut socket = server::Server::detached()?;
    let mut app = server_main::ServerApp { message: None };

    socket
        .add_implementation::<server_main::integration_tests_v1::IntegrationTestProtocolV1Impl, _>(
            INTEGRATION_TESTS_PROTOCOL_VERSION,
            &mut app,
        );

    socket.add_client(server_stream).expect("add_client failed");

    loop {
        let loop_fd = socket.extract_loop_fd();

        let mut pfds = [
            poll::PollFd::new(loop_fd, poll::PollFlags::POLLIN),
            poll::PollFd::new(shutdown_read.as_fd(), poll::PollFlags::POLLIN),
        ];

        poll::poll(&mut pfds, poll::PollTimeout::NONE)?;

        let loop_ready = pfds[0]
            .revents()
            .is_some_and(|r| r.contains(poll::PollFlags::POLLIN));

        let shutdown_ready = pfds[1]
            .revents()
            .is_some_and(|r| r.contains(poll::PollFlags::POLLIN));

        if shutdown_ready {
            break;
        }

        if loop_ready {
            let _ = socket.dispatch_events(&mut app, false);
        }
    }

    unsafe {
        let mut status = 0;
        libc::waitpid(pid, &mut status, 0);
    }

    assert_eq!(Some("Hello!"), app.message.as_deref());

    Ok(())
}
