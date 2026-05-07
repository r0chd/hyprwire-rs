use crate::client::client_socket;
use crate::{message, socket, trace};
use hyprwire_core::message::Message;
use hyprwire_core::message::wire::generic_protocol_message;
use std::os::fd::AsRawFd;
use std::sync::atomic;
use std::{mem, ops, sync, time};

const HANDSHAKE_MAX_MS: u64 = 5000;

/// An event queue
///
/// This is an abstraction for handling event dispatching, that allows you to ensure
/// access to some common state `&mut State` to your event handlers.
///
/// Event queues are created through [`Client::new_event_queue()`].
pub struct EventQueue {
    handle: QueueHandle,
}

/// A handle representing an [`EventQueue`], used to assign objects upon creation.
#[derive(Clone)]
pub struct QueueHandle {
    inner: sync::Arc<sync::Mutex<EventQueueInner>>,
}

#[derive(Clone)]
pub(crate) struct WeakQueueHandle {
    inner: sync::Weak<sync::Mutex<EventQueueInner>>,
}

struct EventQueueInner {
    socket: sync::Arc<client_socket::ClientSocket>,
    queue: Vec<generic_protocol_message::GenericProtocolMessage<ops::Range<usize>>>,
}

impl EventQueue {
    pub(crate) fn new(socket: sync::Arc<client_socket::ClientSocket>) -> Self {
        Self {
            handle: QueueHandle {
                inner: sync::Arc::new(sync::Mutex::new(EventQueueInner {
                    socket,
                    queue: Vec::new(),
                })),
            },
        }
    }

    /// Get a [`QueueHandle`] for this event queue
    pub fn handle(&self) -> QueueHandle {
        self.handle.clone()
    }

    /// Dispatches pending events from the server.
    ///
    /// `state` receives generated event callbacks. If `block` is `true`, this
    /// call waits until new protocol traffic is available.
    ///
    /// # Errors
    /// Returns an error if the connection closes, polling fails, or incoming
    /// protocol traffic is malformed.
    pub fn dispatch_events<D: 'static>(&self, dispatch: &mut D, block: bool) -> crate::Result<()> {
        self.handle.dispatch_events(dispatch, block)
    }

    /// Performs a roundtrip against the server.
    ///
    /// This sends a roundtrip request and blocks until the matching
    /// acknowledgment is received, dispatching events into `state` while
    /// waiting.
    ///
    /// # Errors
    /// Returns an error if the connection closes or dispatching protocol
    /// traffic fails while waiting for the roundtrip acknowledgment.
    pub fn roundtrip<D: 'static>(&self, dispatch: &mut D) -> crate::Result<()> {
        let socket = sync::Arc::clone(&self.handle.inner.lock().unwrap().socket);
        socket.roundtrip(&self.handle, dispatch)
    }

    /// Blocks until the initial Hyprwire handshake completes.
    ///
    /// Returns an error if the connection closes or the handshake fails.
    ///
    /// # Errors
    /// Returns an error if the connection closes, the handshake times out, or
    /// the server sends invalid handshake traffic.
    pub fn wait_for_handshake<D: 'static>(&self, dispatch: &mut D) -> crate::Result<()> {
        let socket = sync::Arc::clone(&self.handle.inner.lock().unwrap().socket);
        socket.wait_for_handshake(&self.handle, dispatch)
    }
}

impl QueueHandle {
    pub(crate) fn downgrade(&self) -> WeakQueueHandle {
        WeakQueueHandle {
            inner: sync::Arc::downgrade(&self.inner),
        }
    }

    pub(crate) fn enqueue(
        &self,
        msg: generic_protocol_message::GenericProtocolMessage<ops::Range<usize>>,
    ) {
        self.inner.lock().unwrap().queue.push(msg);
    }

    pub(crate) fn dispatch_events<D: 'static>(
        &self,
        dispatch: &mut D,
        block: bool,
    ) -> crate::Result<()> {
        let socket = sync::Arc::clone(&self.inner.lock().unwrap().socket);

        if socket.state.error.load(atomic::Ordering::Relaxed) {
            return Err(crate::Error::ConnectionClosed);
        }

        socket.collect_orphaned_objects();

        if !socket.handshake_done.load(atomic::Ordering::Relaxed) {
            #[allow(clippy::cast_possible_truncation)]
            let elapsed_ms = socket.handshake_begin.elapsed().as_millis() as u64;
            let max_ms = HANDSHAKE_MAX_MS.saturating_sub(elapsed_ms);

            let timeout = if block {
                time::Duration::from_millis(max_ms)
            } else {
                time::Duration::ZERO
            };

            let mut events = polling::Events::new();
            if socket.poller.wait(&mut events, Some(timeout))? == 0 {
                if block {
                    socket.disconnect_on_error();
                    return Err(crate::Error::HandshakeTimeout);
                }
                return Ok(());
            }

            socket
                .poller
                .modify(&socket.state.stream, polling::Event::readable(0))?;
        }

        if socket.handshake_done.load(atomic::Ordering::Relaxed) {
            let timeout = if block {
                None
            } else {
                Some(time::Duration::ZERO)
            };

            let mut events = polling::Events::new();
            if socket.poller.wait(&mut events, timeout)? == 0 {
                if block {
                    return Err(crate::Error::ConnectionClosed);
                }
                socket.collect_orphaned_objects();
                return Ok(());
            }

            socket
                .poller
                .modify(&socket.state.stream, polling::Event::readable(0))?;
        }

        let mut data = {
            match socket::SocketRawParsedMessage::read_from_socket(&socket.state.stream) {
                Err(_) => {
                    crate::log_error!("fatal: received malformed message from server");
                    socket.disconnect_on_error();
                    return Err(crate::Error::ConnectionClosed);
                }
                Ok(data) => data,
            }
        };

        if data.data.is_empty() {
            return Err(crate::Error::ConnectionClosed);
        }

        if let Err(e) =
            message::handle_message(&mut data, &message::Role::Client(&socket), dispatch)
        {
            crate::log_error!("fatal: failed to handle message on wire");
            socket.disconnect_on_error();
            return Err(crate::Error::from(e));
        }

        let mut inner = self.inner.lock().unwrap();
        let pending = mem::take(&mut inner.queue);
        for mut msg in pending {
            let seq = msg.depends_on_seq();
            let obj_id = socket
                .object_for_seq(seq)
                .map(|obj| obj.id.load(atomic::Ordering::Relaxed));

            match obj_id {
                None => continue,
                Some(0) => {
                    inner.queue.push(msg);
                    continue;
                }
                Some(id) => {
                    msg.resolve_seq(id);
                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] -> Handle deferred {}", socket.state.stream.as_raw_fd(), crate::steady_millis(), msg.parse_data())
                    }
                }
            }

            socket.state.send_message(&msg);
        }

        drop(inner);

        socket.collect_orphaned_objects();

        if socket.state.error.load(atomic::Ordering::Relaxed) {
            return Err(crate::Error::ConnectionClosed);
        }

        Ok(())
    }
}

impl WeakQueueHandle {
    pub fn upgrade(&self) -> Option<QueueHandle> {
        self.inner.upgrade().map(|inner| QueueHandle { inner })
    }
}
