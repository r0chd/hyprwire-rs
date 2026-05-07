use hyprwire::{client, server};
use nix::poll;
use std::fs;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net;

const INTEGRATION_TESTS_PROTOCOL_VERSION: u32 = 1;

mod server_main {
    pub mod integration_tests_v1 {
        hyprwire::include_protocol!("integration_test_protocol_v1");
        pub use server::*;
    }
    use super::*;
    use integration_tests_v1::{integration_manager_v1, integration_object_v1};

    pub struct ServerApp {
        pub message: Option<String>,
        pub should_exit: bool,
    }

    impl hyprwire::Dispatch<integration_manager_v1::IntegrationManagerV1> for ServerApp {
        fn event(
            &mut self,
            object: &integration_manager_v1::IntegrationManagerV1,
            event: <integration_manager_v1::IntegrationManagerV1 as hyprwire::Object>::Event<'_>,
        ) {
            match event {
                integration_manager_v1::Event::SendMessage { message } => {
                    object.send_recv_message(&message);
                    self.message = Some(message);
                }
                integration_manager_v1::Event::SendUint { value } => {
                    object.send_recv_uint(value);
                }
                integration_manager_v1::Event::SendInt { value } => {
                    object.send_recv_int(value);
                }
                integration_manager_v1::Event::SendFloat { value } => {
                    object.send_recv_float(value);
                }
                integration_manager_v1::Event::SendFd { value } => {
                    object.send_recv_fd(value);
                }
                integration_manager_v1::Event::SendArrayUint { values } => {
                    object.send_recv_array_uint(&values);
                }
                integration_manager_v1::Event::SendArrayString { values } => {
                    object.send_recv_array_string(&values);
                }
                integration_manager_v1::Event::SendArrayFd { values } => {
                    object.send_recv_array_fd(&values);
                }
                integration_manager_v1::Event::SendStart { cmd, env } => {
                    object.send_recv_start(&cmd, &env);
                }
                integration_manager_v1::Event::SendMixed { a, b, c, d } => {
                    object.send_recv_mixed(a, &b, c, &d);
                }
                integration_manager_v1::Event::SendEnum { value } => {
                    object.send_recv_enum(value);
                }
                integration_manager_v1::Event::MakeObject { .. } => {}
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
                &mut app,
                INTEGRATION_TESTS_PROTOCOL_VERSION,
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

    hyprwire::delegate_noop!(ServerApp: integration_object_v1::IntegrationObjectV1);
}

mod client_main {
    mod integration_tests_v1 {
        hyprwire::include_protocol!("integration_test_protocol_v1");
        pub use client::*;
        pub use spec::TestEnum;
    }
    use super::*;
    use integration_tests_v1::integration_manager_v1;

    #[derive(Default)]
    struct ClientApp {
        received_message: Option<String>,
        received_uint: Option<u32>,
        received_int: Option<i32>,
        received_float: Option<f32>,
        received_fd: Option<OwnedFd>,
        received_array_uint: Option<Vec<u32>>,
        received_array_string: Option<Vec<String>>,
        received_array_fd: Option<Vec<OwnedFd>>,
        received_start: Option<(Vec<String>, Vec<String>)>,
        received_mixed: Option<(u32, String, OwnedFd, Vec<u32>)>,
        received_enum: Option<integration_tests_v1::TestEnum>,
    }

    impl hyprwire::Dispatch<integration_manager_v1::IntegrationManagerV1> for ClientApp {
        fn event(
            &mut self,
            _object: &integration_manager_v1::IntegrationManagerV1,
            event: <integration_manager_v1::IntegrationManagerV1 as hyprwire::Object>::Event<'_>,
        ) {
            match event {
                integration_tests_v1::client::integration_manager_v1::Event::RecvMessage {
                    message,
                } => {
                    self.received_message = Some(message);
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvUint { value } => {
                    self.received_uint = Some(value);
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvInt { value } => {
                    self.received_int = Some(value);
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvFloat {
                    value,
                } => {
                    self.received_float = Some(value);
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvFd { value } => {
                    self.received_fd = Some(value);
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvArrayUint {
                    values,
                } => {
                    self.received_array_uint = Some(values);
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvArrayString {
                    values,
                } => {
                    self.received_array_string = Some(values);
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvArrayFd {
                    values,
                } => {
                    self.received_array_fd = Some(values);
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvStart {
                    cmd,
                    env,
                } => {
                    self.received_start = Some((cmd, env));
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvMixed {
                    a,
                    b,
                    c,
                    d,
                } => {
                    self.received_mixed = Some((a, b, c, d));
                }
                integration_tests_v1::client::integration_manager_v1::Event::RecvEnum { value } => {
                    self.received_enum = Some(value);
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

    fn socket_pair_with_data(data: &[u8]) -> (net::UnixStream, net::UnixStream) {
        let (local, remote) = net::UnixStream::pair().unwrap();
        (&local).write_all(data).unwrap();
        (local, remote)
    }

    fn read_exact_from_fd(fd: OwnedFd, n: usize) -> Vec<u8> {
        let mut buf = vec![0u8; n];
        fs::File::from(fd).read_exact(&mut buf).unwrap();
        buf
    }

    pub fn main(client_stream: net::UnixStream) -> hyprwire::Result<()> {
        let mut socket = client::Client::from_fd(client_stream)?;
        let event_queue = socket.new_event_queue();
        let qh = event_queue.handle();
        let mut app = ClientApp::default();

        socket.add_implementation::<integration_tests_v1::IntegrationTestProtocolV1Impl>();
        event_queue.wait_for_handshake(&mut app)?;

        let manager = socket.bind::<integration_manager_v1::IntegrationManagerV1, ClientApp>(
            &qh,
            &mut app,
            INTEGRATION_TESTS_PROTOCOL_VERSION,
        )?;

        // varchar
        manager.send_send_message("Hello!");
        event_queue.roundtrip(&mut app)?;
        assert_eq!(app.received_message.take().as_deref(), Some("Hello!"));

        // uint
        manager.send_send_uint(0xDEAD_BEEF);
        event_queue.roundtrip(&mut app)?;
        assert_eq!(app.received_uint.take(), Some(0xDEAD_BEEF));

        // int
        manager.send_send_int(-42);
        event_queue.roundtrip(&mut app)?;
        assert_eq!(app.received_int.take(), Some(-42));

        // f32
        manager.send_send_float(1.5);
        event_queue.roundtrip(&mut app)?;
        assert_eq!(app.received_float.take(), Some(1.5f32));

        // fd
        let (local_a, remote_a) = socket_pair_with_data(b"fdtest");
        manager.send_send_fd(remote_a);
        event_queue.roundtrip(&mut app)?;
        assert_eq!(
            read_exact_from_fd(app.received_fd.take().unwrap(), 6),
            b"fdtest"
        );
        drop(local_a);

        // array uint
        manager.send_send_array_uint(&[1, 2, 3, 4, 5]);
        event_queue.roundtrip(&mut app)?;
        assert_eq!(app.received_array_uint.take(), Some(vec![1, 2, 3, 4, 5]));

        // array string
        manager.send_send_array_string(&["foo", "bar", "baz"]);
        event_queue.roundtrip(&mut app)?;
        assert_eq!(
            app.received_array_string.take(),
            Some(vec![
                "foo".to_string(),
                "bar".to_string(),
                "baz".to_string()
            ])
        );

        // array fd
        let (local_b, remote_b) = socket_pair_with_data(b"fd0");
        let (local_c, remote_c) = socket_pair_with_data(b"fd1");
        manager.send_send_array_fd(&[remote_b, remote_c]);
        event_queue.roundtrip(&mut app)?;
        let fds = app.received_array_fd.take().unwrap();
        assert_eq!(fds.len(), 2);
        assert_eq!(
            read_exact_from_fd(fds.into_iter().next().unwrap(), 3),
            b"fd0"
        );
        drop(local_b);
        drop(local_c);

        // send_start (two varchar arrays)
        manager.send_send_start(&["bash", "-c", "echo"], &["HOME=/tmp", "PATH=/bin"]);
        event_queue.roundtrip(&mut app)?;
        let (cmd, env) = app.received_start.take().unwrap();
        assert_eq!(cmd, ["bash", "-c", "echo"]);
        assert_eq!(env, ["HOME=/tmp", "PATH=/bin"]);

        // mixed (uint + varchar + fd + array uint)
        let (local_d, remote_d) = socket_pair_with_data(b"mix");
        manager.send_send_mixed(77, "hello", remote_d, &[10, 20, 30]);
        event_queue.roundtrip(&mut app)?;
        let (a, b, c, d) = app.received_mixed.take().unwrap();
        assert_eq!(a, 77);
        assert_eq!(b, "hello");
        assert_eq!(read_exact_from_fd(c, 3), b"mix");
        assert_eq!(d, [10, 20, 30]);
        drop(local_d);

        // enum
        manager.send_send_enum(integration_tests_v1::TestEnum::World);
        event_queue.roundtrip(&mut app)?;
        assert_eq!(
            app.received_enum.take(),
            Some(integration_tests_v1::TestEnum::World)
        );

        manager.send_shutdown();
        event_queue.roundtrip(&mut app)?;

        Ok(())
    }
}

#[test]
fn integration_protocol_roundtrip() -> hyprwire::Result<()> {
    let (server_stream, client_stream) = net::UnixStream::pair()?;

    let server = std::thread::spawn(move || server_main::main(server_stream));

    client_main::main(client_stream)?;

    server.join().expect("server thread panicked")?;

    Ok(())
}
