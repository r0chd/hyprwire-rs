use hyprwire::{client, server};
use nix::poll;
use std::os::unix::net;
use std::process;

const INTEGRATION_TESTS_PROTOCOL_VERSION: u32 = 1;

mod server_main {
    pub mod integration_tests_v1 {
        hyprwire::include_protocol!("integration_test_protocol_v1");
        pub use server::*;
    }
    use super::*;
    use integration_tests_v1::integration_manager_v1;

    pub struct ServerApp {
        pub message: Option<String>,
        pub should_exit: bool,
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
                integration_manager_v1::Event::Shutdown => {
                    self.should_exit = true;
                }
            }
        }
    }

    impl integration_tests_v1::IntegrationTestProtocolV1Handler for ServerApp {
        fn bind(&mut self, _object: integration_manager_v1::IntegrationManagerV1) {}
    }

    pub fn main(server_stream: net::UnixStream) -> hyprwire::Result<()> {
        let mut socket = server::Server::detached()?;
        let mut app = ServerApp {
            message: None,
            should_exit: false,
        };

        socket
            .add_implementation::<server_main::integration_tests_v1::IntegrationTestProtocolV1Impl, _>(
                INTEGRATION_TESTS_PROTOCOL_VERSION,
                &mut app,
            );

        socket.add_client(server_stream).expect("add_client failed");

        loop {
            let loop_fd = socket.extract_loop_fd();
            let mut pfds = [poll::PollFd::new(loop_fd, poll::PollFlags::POLLIN)];
            poll::poll(&mut pfds, poll::PollTimeout::NONE).unwrap();

            if pfds[0]
                .revents()
                .is_some_and(|r| r.contains(poll::PollFlags::POLLIN))
            {
                let _ = socket.dispatch_events(&mut app, false);
            }

            if app.should_exit {
                break;
            }
        }

        assert_eq!(Some("Hello!"), app.message.as_deref());

        Ok(())
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

    pub fn main(client_stream: net::UnixStream) -> hyprwire::Result<()> {
        let mut socket = client::Client::from_fd(client_stream)?;
        let mut app = ClientApp;

        socket.add_implementation::<integration_tests_v1::IntegrationTestProtocolV1Impl>();
        socket.wait_for_handshake(&mut app)?;

        let spec = socket
            .get_spec::<integration_tests_v1::IntegrationTestProtocolV1Impl>()
            .unwrap();

        let manager = socket.bind::<integration_manager_v1::IntegrationManagerV1, ClientApp>(
            &spec,
            INTEGRATION_TESTS_PROTOCOL_VERSION,
            &mut app,
        )?;

        manager.send_send_message("Hello!");
        socket.roundtrip(&mut app)?;

        manager.send_shutdown();
        socket.roundtrip(&mut app)?;

        Ok(())
    }
}

#[test]
fn integration_protocol_roundtrip() -> hyprwire::Result<()> {
    let (server_stream, client_stream) = net::UnixStream::pair()?;

    let pid = unsafe { nix::libc::fork() };
    if pid < 0 {
        return Err(std::io::Error::other("fork failed").into());
    }

    if pid == 0 {
        drop(client_stream);

        if let Err(err) = server_main::main(server_stream) {
            eprintln!("server error: {err}");
            process::exit(1);
        }

        process::exit(0);
    }

    drop(server_stream);

    if let Err(err) = client_main::main(client_stream) {
        eprintln!("client error: {err}");
    }

    let mut status = 0;
    unsafe { nix::libc::waitpid(pid, &mut status, 0) };

    assert!(
        nix::libc::WIFEXITED(status) && nix::libc::WEXITSTATUS(status) == 0,
        "server process exited with status {status}"
    );

    Ok(())
}
