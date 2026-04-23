use criterion::Criterion;
use hyprwire::{client, server};
use nix::{libc, poll};
use std::hint::black_box;
use std::io;
use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::net;
use std::{path, process};

const BENCH_PROTOCOL_VERSION: u32 = 1;

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

fn client_lifecycle(socket_path: &path::Path) -> hyprwire::Result<()> {
    let mut socket = client::Client::connect(socket_path).map_err(hyprwire::Error::Io)?;
    let mut app = ClientApp;

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

    manager.send_send_message(black_box("Hello!"));
    socket.roundtrip(&mut app)?;

    Ok(())
}

fn client_process_main(
    socket_path: &path::Path,
    mut shutdown_write: net::UnixStream,
) -> hyprwire::Result<()> {
    let long_message = make_lorem_ipsum(LONG_MESSAGE_TARGET_BYTES);
    assert!(long_message.len() >= LONG_MESSAGE_TARGET_BYTES);

    let mut c = Criterion::default().configure_from_args();

    c.bench_function("client_connect_disconnect", |b| {
        b.iter(|| {
            client_lifecycle(socket_path).unwrap();
        })
    });

    c.final_summary();

    let _ = shutdown_write.write_all(b"x");

    Ok(())
}

fn main() -> io::Result<()> {
    let socket_path = path::PathBuf::from(format!(
        "/tmp/hyprwire-bench-{}-{}",
        process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let (shutdown_read, shutdown_write) = net::UnixStream::pair()?;

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::other("fork failed"));
    }

    if pid == 0 {
        drop(shutdown_read);

        // Wait briefly for the server to bind the socket.
        while !socket_path.exists() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        if let Err(err) = client_process_main(&socket_path, shutdown_write) {
            eprintln!("client error: {err}");
            process::exit(1);
        }
        process::exit(0);
    }

    drop(shutdown_write);

    // Server
    let mut socket = server::Server::open(Some(&socket_path))?;
    let mut app = ServerApp;
    socket.add_implementation::<bench_protocol_v1::s::BenchProtocolV1Impl, _>(
        BENCH_PROTOCOL_VERSION,
        &mut app,
    );

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

    let _ = std::fs::remove_file(&socket_path);

    unsafe {
        let mut status = 0;
        libc::waitpid(pid, &mut status, 0);
    }

    Ok(())
}
