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
use test_protocol_v1::my_manager_v1;

#[derive(Default)]
struct App;

impl hyprwire::Dispatch<my_manager_v1::Object> for App {
    fn event(
        &mut self,
        _object: &my_manager_v1::Object,
        event: <my_manager_v1::Object as hyprwire::Object>::Event<'_>,
    ) {
        if let my_manager_v1::Event::SendMessage { message } = event {
            println!("server says {message}");
        }
    }
}

fn main() -> io::Result<()> {
    // Connect to the server.
    let mut client = client::Client::connect("/tmp/test-hw.sock")?;
    let mut app = App::default();

    // Register the generated client-side implementation so incoming events
    // can be decoded into typed callbacks.
    client.add_implementation::<test_protocol_v1::TestProtocolV1Impl>();

    // Finish protocol negotiation.
    client.wait_for_handshake(&mut app)?;

    // Look up the protocol advertised by the server.
    let server_spec = client
        .get_spec::<test_protocol_v1::TestProtocolV1Impl>()
        .expect("protocol unsupported");

    // Bind the protocol's root object.
    let manager = client
        .bind::<my_manager_v1::Object, App>(&server_spec, server_spec.spec_ver(), &mut app)?;

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
use test_protocol_v1::my_manager_v1;

#[derive(Default)]
struct App;

impl hyprwire::Dispatch<my_manager_v1::Object> for App {
    fn event(
        &mut self,
        _object: &my_manager_v1::Object,
        event: <my_manager_v1::Object as hyprwire::Object>::Event<'_>,
    ) {
        if let my_manager_v1::Event::SendMessage { message } = event {
            println!("client says {message}");
        }
    }
}

impl test_protocol_v1::TestProtocolV1Handler for App {
    fn bind(&mut self, object: my_manager_v1::Object) {
        object.send_send_message("hello from server");
    }
}

fn main() -> std::io::Result<()> {
    let mut app = App;

    // Create a listening server socket.
    let mut server = Server::bind(path::Path::new("/tmp/test-hw.sock"))?;

    // Register the generated server-side implementation.
    server.add_implementation::<test_protocol_v1::TestProtocolV1Impl, _>(1, &mut app);

    // Block and dispatch client requests forever.
    while server.dispatch_events(&mut app, true) {}
    Ok(())
}
```

## Crates

| Crate | Description |
|---|---|
| [`hyprwire`](hyprwire) | Client/server runtime, transport over Unix sockets |
| [`hyprwire-core`](hyprwire-core) | Wire format, message types, protocol traits (`no_std + alloc`) |
| [`hyprwire-scanner`](hyprwire-scanner) | codegen, turns protocol XML into Rust bindings |
| [`hyprwire-protocols`](hyprwire-protocols) | Protocol definitions (placeholder, waiting for [hyprwm/hyprwire-protocols#1](https://github.com/hyprwm/hyprwire-protocols/pull/1)) |

## Examples

You can check out examples in:

- [`hyprwire/examples/basic/client.rs`](hyprwire/examples/basic/client.rs)
- [`hyprwire/examples/basic/server.rs`](hyprwire/examples/basic/server.rs)
- [`hyprwire/examples/calloop/client.rs`](hyprwire/examples/calloop/client.rs)
- [`hyprwire/examples/calloop/server.rs`](hyprwire/examples/calloop/server.rs)
- [`hyprwire/examples/fork/main.rs`](hyprwire/examples/fork/main.rs)
