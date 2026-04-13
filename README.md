# hyprwire

`hyprwire-rs` is a pure Rust implementation of Hyprwire wire protocol.

## Code Generation

Bindings are generated at build time from protocol XML files.

Add the dependencies first:

```sh
cargo add hyprwire
cargo add --build hyprwire-scanner
```

Example `build.rs`:

By default, `configure()` enables all generation targets, so both client and
server bindings are emitted.

If you only want a subset, use `with_targets(...)`:

```rust
fn main() {
    hyprwire_scanner::configure()
        .with_targets(hyprwire_scanner::Targets::CLIENT | hyprwire_scanner::Targets::SERVER)
        .compile(&["examples/protocols/protocol-v1.xml"])
        .unwrap();
}
```

Then include the generated module:

```rust
mod test_protocol_v1 {
    hyprwire::include_protocol!("test_protocol_v1");
}
```

Generated modules contain:

- `client` bindings
- `server` bindings
- protocol/object/enum specs

## Client Flow

Minimal example:

```rust
mod test_protocol_v1 {
    hyprwire::include_protocol!("test_protocol_v1");
    pub use client::*;
}

use std::{path, io};
use hyprwire::client;
use hyprwire::implementation::client::ProtocolImplementations;
use hyprwire::implementation::types::ProtocolSpec;

#[derive(Default)]
struct App;

impl hyprwire::Dispatch<test_protocol_v1::MyManagerV1Object> for App {
    fn event(
        &mut self,
        _object: &test_protocol_v1::MyManagerV1Object,
        event: <test_protocol_v1::MyManagerV1Object as hyprwire::Object>::Event<'_>,
    ) {
        if let test_protocol_v1::MyManagerV1Event::SendMessage { message } = event {
            println!("server says {message}");
        }
    }
}

fn main() -> io::Result<()> {
    // Connect to the server.
    let mut client = client::Client::open(path::Path::new("/tmp/test-hw.sock"))?;

    // Register the generated client-side implementation so incoming events
    // can be decoded into typed callbacks.
    let implementation = test_protocol_v1::TestProtocolV1Impl::default();
    client.add_implementation(implementation.clone());

    // Finish protocol negotiation.
    client.wait_for_handshake(&mut state)?;

    // Look up the protocol advertised by the server.
    let spec = client
        .get_spec(implementation.protocol().spec_name())
        .expect("protocol unsupported");

    let mut app = App;

    // Bind the protocol's root object.
    let manager = client
        .bind::<test_protocol_v1::MyManagerV1Object, App>(&spec, 1)?;

    manager.send_send_message("hello");

    // Dispatch server events into `app`.
    client.dispatch_events(&mut app, true)?;
    Ok(())
}
```

## Server Flow

The generated server module includes a protocol handler trait. Its `bind`
method is called whenever the root object for that protocol is bound for a
client.

Minimal example:

```rust
mod test_protocol_v1 {
    hyprwire::include_protocol!("test_protocol_v1");
    pub use server::*;
}

use std::path;
use hyprwire::server::Server;

#[derive(Default)]
struct App;

impl hyprwire::Dispatch<test_protocol_v1::MyManagerV1Object> for App {
    fn event(
        &mut self,
        _object: &test_protocol_v1::MyManagerV1Object,
        event: <test_protocol_v1::MyManagerV1Object as hyprwire::Object>::Event<'_>,
    ) {
        if let test_protocol_v1::MyManagerV1Event::SendMessage { message } = event {
            println!("client says {message}");
        }
    }
}

impl test_protocol_v1::TestProtocolV1Handler for App {
    fn bind(&mut self, object: test_protocol_v1::MyManagerV1Object) {
        object.send_send_message("hello from server");
    }
}

fn main() -> std::io::Result<()> {
    let mut app = App;

    // Create a listening server socket.
    let mut server = Server::open(Some(path::Path::new("/tmp/test-hw.sock")))?;

    // Register the generated server-side implementation.
    let implementation = test_protocol_v1::TestProtocolV1Impl::new(1, &mut app);
    server.add_implementation(implementation);

    // Block and dispatch client requests forever.
    while server.dispatch_events(&mut app, true) {}
    Ok(())
}
```

## Examples

The repository is organized as a Cargo workspace with:

- [`hyprwire/`](hyprwire)
- [`hyprwire-scanner/`](hyprwire-scanner)
- [`hyprwire-protocols/`](hyprwire-protocols)

`hyprwire-protocols` is currently a placeholder crate, waiting for
https://github.com/hyprwm/hyprwire-protocols/pull/1.

You can check out examples in:

- [`hyprwire/examples/basic/client.rs`](hyprwire/examples/basic/client.rs)
- [`hyprwire/examples/basic/server.rs`](hyprwire/examples/basic/server.rs)
- [`hyprwire/examples/calloop/client.rs`](hyprwire/examples/calloop/client.rs)
- [`hyprwire/examples/calloop/server.rs`](hyprwire/examples/calloop/server.rs)
- [`hyprwire/examples/fork/main.rs`](hyprwire/examples/fork/main.rs)
