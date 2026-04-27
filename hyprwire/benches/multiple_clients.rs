use criterion::Criterion;
use hyprwire::{client, server};
use nix::{libc, poll};
use std::hint::black_box;
use std::io;
use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::net;
use std::process;

const BENCH_PROTOCOL_VERSION: u32 = 1;
const CLIENTS: usize = 100;

mod bench_protocol_v1 {
    hyprwire::include_protocol!("bench_protocol_v1");
    pub use client as c;
    pub use server as s;
}

const LONG_MESSAGE_TARGET_BYTES: usize = 8192;

fn make_lorem_ipsum(min_bytes: usize) -> String {
    const P: &str = concat!(
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\n",
        "Curabitur lobortis tellus non neque vestibulum, id porttitor orci scelerisque.\n",
        "Vivamus laoreet volutpat mattis. Cras ornare enim ultrices tellus pellentesque molestie.\n",
        "Sed et turpis enim. Proin convallis faucibus felis, ac cursus nibh volutpat a.\n",
        "Vestibulum sed enim dictum, tempus urna non, elementum.\n",
        "\n",
    );

    let mut out = String::with_capacity(min_bytes + P.len());
    while out.len() < min_bytes {
        out.push_str(P);
    }
    out
}

struct ServerApp;

hyprwire::delegate_noop!(ServerApp: ignore bench_protocol_v1::s::bench_v1::BenchV1);
hyprwire::delegate_noop!(ClientApp: ignore bench_protocol_v1::c::bench_v1::BenchV1);

impl bench_protocol_v1::s::BenchProtocolV1Handler for ServerApp {
    fn bind(&mut self, _object: bench_protocol_v1::s::bench_v1::BenchV1) {}
}

struct ClientApp;

fn client_process_main(
    server_streams: Vec<net::UnixStream>,
    mut shutdown_write: net::UnixStream,
) -> hyprwire::Result<()> {
    let mut app = ClientApp;

    let long_message = make_lorem_ipsum(LONG_MESSAGE_TARGET_BYTES);
    assert!(long_message.len() >= LONG_MESSAGE_TARGET_BYTES);

    let mut clients = Vec::with_capacity(server_streams.len());
    let mut managers = Vec::with_capacity(server_streams.len());
    let mut specs = Vec::with_capacity(server_streams.len());

    for stream in server_streams {
        let mut socket = client::Client::from_fd(stream).map_err(hyprwire::Error::Io)?;

        socket.add_implementation::<bench_protocol_v1::c::BenchProtocolV1Impl>();
        socket.wait_for_handshake(&mut app)?;

        let spec = socket
            .get_spec::<bench_protocol_v1::c::BenchProtocolV1Impl>()
            .ok_or(hyprwire::Error::ProtocolViolation(
                hyprwire::core::message::Error::NoSpec,
            ))?;

        let manager = socket.bind::<bench_protocol_v1::c::bench_v1::BenchV1, ClientApp>(
            &spec,
            BENCH_PROTOCOL_VERSION,
            &mut app,
        )?;

        managers.push(manager);
        specs.push(spec);
        clients.push(socket);
    }

    let mut c = Criterion::default().configure_from_args();

    c.bench_function("multi_100_clients_send_message", |b| {
        b.iter(|| {
            for manager in &managers {
                manager.send_send_message(black_box("Hello!"));
            }
        })
    });

    for socket in &clients {
        socket.roundtrip(&mut app).unwrap();
    }

    c.bench_function("multi_100_clients_send_message+roundtrip", |b| {
        b.iter(|| {
            for (socket, manager) in clients.iter().zip(&managers) {
                manager.send_send_message(black_box("Hello!"));
                socket.roundtrip(&mut app).unwrap();
            }
        })
    });

    c.bench_function("multi_100_clients_send_message_long", |b| {
        b.iter(|| {
            for manager in &managers {
                manager.send_send_message(black_box(long_message.as_str()));
            }
        })
    });

    for socket in &clients {
        socket.roundtrip(&mut app).unwrap();
    }

    c.bench_function("multi_100_clients_send_message_long+roundtrip", |b| {
        b.iter(|| {
            for (socket, manager) in clients.iter().zip(&managers) {
                manager.send_send_message(black_box(long_message.as_str()));
                socket.roundtrip(&mut app).unwrap();
            }
        })
    });

    c.bench_function("multi_100_clients_bind_object", |b| {
        b.iter(|| {
            for (socket, spec) in clients.iter().zip(&specs) {
                let _ = black_box(
                    socket
                        .bind::<bench_protocol_v1::c::bench_v1::BenchV1, ClientApp>(
                            spec,
                            BENCH_PROTOCOL_VERSION,
                            &mut app,
                        )
                        .unwrap(),
                );
            }
        })
    });

    c.final_summary();

    // Tell the server process to stop its event loop.
    let _ = shutdown_write.write_all(b"x");

    Ok(())
}

fn main() -> io::Result<()> {
    let mut server_streams = Vec::with_capacity(CLIENTS);
    let mut client_streams = Vec::with_capacity(CLIENTS);

    for _ in 0..CLIENTS {
        let (server_stream, client_stream) = net::UnixStream::pair()?;
        server_streams.push(server_stream);
        client_streams.push(client_stream);
    }

    let (shutdown_read, shutdown_write) = net::UnixStream::pair()?;

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::other("fork failed"));
    }

    if pid == 0 {
        drop(shutdown_read);

        drop(server_streams);

        if let Err(err) = client_process_main(client_streams, shutdown_write) {
            eprintln!("client error: {err}");
            process::exit(1);
        }
        process::exit(0);
    }

    drop(client_streams);
    drop(shutdown_write);

    // Server

    let mut socket = server::Server::detached()?;
    let mut app = ServerApp;
    socket.add_implementation::<bench_protocol_v1::s::BenchProtocolV1Impl, _>(
        BENCH_PROTOCOL_VERSION,
        &mut app,
    );

    for server_stream in server_streams {
        socket.add_client(server_stream).expect("add_client failed");
    }

    loop {
        let (loop_ready, shutdown_ready) = {
            let loop_fd = socket.extract_loop_fd()?;
            let mut pfds = [
                poll::PollFd::new(loop_fd, poll::PollFlags::POLLIN),
                poll::PollFd::new(shutdown_read.as_fd(), poll::PollFlags::POLLIN),
            ];

            let _ = poll::poll(&mut pfds, poll::PollTimeout::NONE)?;
            let loop_ready = pfds[0]
                .revents()
                .is_some_and(|revents| revents.contains(poll::PollFlags::POLLIN));
            let shutdown_ready = pfds[1]
                .revents()
                .is_some_and(|revents| revents.contains(poll::PollFlags::POLLIN));
            (loop_ready, shutdown_ready)
        };

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

    Ok(())
}
