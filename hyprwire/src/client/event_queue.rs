use crate::client::client_socket;
use crate::{socket, trace};
use hyprwire_core::message::Message;
use hyprwire_core::message::wire::{generic_protocol_message, roundtrip_request};
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
#[derive(Clone)]
pub struct EventQueue {
    pub(crate) inner: sync::Arc<sync::Mutex<EventQueueInner>>,
}

#[derive(Clone)]
pub struct WeakEventQueue {
    inner: sync::Weak<sync::Mutex<EventQueueInner>>,
}

pub struct EventQueueInner {
    socket: sync::Arc<client_socket::ClientSocket>,
    pub(crate) queue: Vec<generic_protocol_message::GenericProtocolMessage<ops::Range<usize>>>,
    last_sent_roundtrip_seq: u32,
    pub(crate) last_ackd_roundtrip_seq: u32,
}

impl EventQueue {
    pub(crate) fn new(socket: sync::Arc<client_socket::ClientSocket>) -> Self {
        Self {
            inner: sync::Arc::new(sync::Mutex::new(EventQueueInner {
                socket,
                queue: Vec::new(),
                last_sent_roundtrip_seq: 0,
                last_ackd_roundtrip_seq: 0,
            })),
        }
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
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(sync::PoisonError::into_inner);
        self.dispatch_events_inner(dispatch, block, &mut inner)
    }

    fn dispatch_events_inner<D: 'static>(
        &self,
        dispatch: &mut D,
        block: bool,
        inner: &mut EventQueueInner,
    ) -> crate::Result<()> {
        if inner.socket.state.error.load(atomic::Ordering::Relaxed) {
            return Err(crate::Error::ConnectionClosed);
        }

        inner.socket.collect_orphaned_objects();

        if !inner.socket.handshake_done.load(atomic::Ordering::Relaxed) {
            #[allow(clippy::cast_possible_truncation)]
            let elapsed_ms = inner.socket.handshake_begin.elapsed().as_millis() as u64;
            let max_ms = HANDSHAKE_MAX_MS.saturating_sub(elapsed_ms);

            let timeout = if block {
                time::Duration::from_millis(max_ms)
            } else {
                time::Duration::ZERO
            };

            let mut events = polling::Events::new();
            if inner.socket.poller.wait(&mut events, Some(timeout))? == 0 {
                if block {
                    inner.socket.disconnect_on_error();
                    return Err(crate::Error::HandshakeTimeout);
                }
                return Ok(());
            }

            inner
                .socket
                .poller
                .modify(&inner.socket.state.stream, polling::Event::readable(0))?;
        }

        if inner.socket.handshake_done.load(atomic::Ordering::Relaxed) {
            let timeout = if block {
                None
            } else {
                Some(time::Duration::ZERO)
            };

            let mut events = polling::Events::new();
            if inner.socket.poller.wait(&mut events, timeout)? == 0 {
                inner.socket.collect_orphaned_objects();
                if block {
                    return Err(crate::Error::ConnectionClosed);
                }
                return Ok(());
            }

            inner
                .socket
                .poller
                .modify(&inner.socket.state.stream, polling::Event::readable(0))?;
        }

        let mut data = {
            match socket::SocketRawParsedMessage::read_from_socket(&inner.socket.state.stream) {
                Err(_) => {
                    crate::log_error!("fatal: received malformed message from server");
                    inner.socket.disconnect_on_error();
                    return Err(crate::Error::ConnectionClosed);
                }
                Ok(data) => data,
            }
        };

        if data.data.is_empty() {
            return Err(crate::Error::ConnectionClosed);
        }

        let socket = sync::Arc::clone(&inner.socket);
        if let Err(e) = socket.handle_message(&mut data, dispatch, inner) {
            crate::log_error!("fatal: failed to handle message on wire");
            inner.socket.disconnect_on_error();
            return Err(crate::Error::from(e));
        }

        let pending = mem::take(&mut inner.queue);
        for mut msg in pending {
            let seq = msg.depends_on_seq();
            let obj_id = inner
                .socket
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
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] -> Handle deferred {}", inner.socket.state.stream.as_raw_fd(), crate::steady_millis(), msg.parse_data())
                    }
                }
            }

            inner.socket.state.send_message(&msg);
        }

        inner.socket.collect_orphaned_objects();

        if inner.socket.state.error.load(atomic::Ordering::Relaxed) {
            return Err(crate::Error::ConnectionClosed);
        }

        Ok(())
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
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(sync::PoisonError::into_inner);

        if inner.socket.state.error.load(atomic::Ordering::Relaxed) {
            return Err(crate::Error::ConnectionClosed);
        }

        inner.last_sent_roundtrip_seq += 1;
        inner
            .socket
            .state
            .send_message(&roundtrip_request::RoundtripRequest::new(
                inner.last_sent_roundtrip_seq,
            ));

        while inner.last_ackd_roundtrip_seq < inner.last_sent_roundtrip_seq {
            self.dispatch_events_inner(dispatch, true, &mut inner)?;
        }

        Ok(())
    }

    /// Blocks until the initial Hyprwire handshake completes.
    ///
    /// Returns an error if the connection closes or the handshake fails.
    ///
    /// # Errors
    /// Returns an error if the connection closes, the handshake times out, or
    /// the server sends invalid handshake traffic.
    pub fn wait_for_handshake<D: 'static>(&self, dispatch: &mut D) -> crate::Result<()> {
        let socket = sync::Arc::clone(
            &self
                .inner
                .lock()
                .unwrap_or_else(sync::PoisonError::into_inner)
                .socket,
        );
        while !socket.state.error.load(atomic::Ordering::Relaxed)
            && !socket.handshake_done.load(atomic::Ordering::Relaxed)
        {
            self.dispatch_events(dispatch, true)?;
        }

        if socket.state.error.load(atomic::Ordering::Relaxed) {
            return Err(crate::Error::ConnectionClosed);
        }

        Ok(())
    }

    pub(crate) fn downgrade(&self) -> WeakEventQueue {
        WeakEventQueue {
            inner: sync::Arc::downgrade(&self.inner),
        }
    }
}

impl WeakEventQueue {
    pub fn upgrade(&self) -> Option<EventQueue> {
        self.inner.upgrade().map(|inner| EventQueue { inner })
    }
}
