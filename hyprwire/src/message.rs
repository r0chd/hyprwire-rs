use crate::client::{client_socket, event_queue};
use crate::server::server_client;
use crate::{socket, steady_millis, trace};
use hyprwire_core::message;
use hyprwire_core::message::Message;
use hyprwire_core::message::wire::{
    bind_protocol, fatal_protocol_error, generic_protocol_message, handshake_ack, handshake_begin,
    handshake_protocols, hello, new_object, roundtrip_done, roundtrip_request,
};
use std::os::fd::AsRawFd;
use std::sync::atomic;

impl client_socket::ClientSocket {
    pub(crate) fn handle_message<D: 'static>(
        &self,
        raw: &mut socket::SocketRawParsedMessage,
        dispatch: &mut D,
        inner: &mut event_queue::EventQueueInner,
    ) -> Result<(), message::Error> {
        let mut needle = 0;
        while needle < raw.data.len() {
            let Ok(message) = message::MessageType::try_from(raw.data[needle]) else {
                crate::log_error!(
                    "server at fd {} core protocol error: invalid message recvd (invalid type code)",
                    self.state.stream.as_raw_fd()
                );
                return Err(message::Error::InvalidMessage);
            };

            needle += match message {
                message::MessageType::HandshakeBegin => {
                    let msg = handshake_begin::HandshakeBegin::from_bytes(&raw.data, needle)
                        .inspect_err(|_| {
                            crate::log_error!(
                                "server at fd {} core protocol error...",
                                self.state.stream.as_raw_fd()
                            );
                        })?;

                    if !msg.versions().contains(&crate::PROTOCOL_VERSION) {
                        crate::log_error!(
                            "server at fd {} core protocol error: version negotiation failed",
                            self.state.stream.as_raw_fd()
                        );
                        self.state.error.store(true, atomic::Ordering::Relaxed);
                        return Err(message::Error::VersionNegotiationFailed);
                    }

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] -> parse error: {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    self.state
                        .send_message(&handshake_ack::HandshakeAck::new(crate::PROTOCOL_VERSION));

                    Ok(msg.data().len())
                }
                message::MessageType::HandshakeProtocols => {
                    let msg = handshake_protocols::HandshakeProtocols::from_bytes(&raw.data, needle)
                        .inspect_err(|_| {
                            crate::log_error!(
                                "server at fd {} core protocol error: malformed message recvd (HandshakeProtocols)",
                                self.state.stream.as_raw_fd()
                            );
                        })?;

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    self.server_specs(msg.protocols());
                    self.handshake_done.store(true, atomic::Ordering::Relaxed);

                    Ok(msg.data().len())
                }
                message::MessageType::NewObject => {
                    let msg = new_object::NewObject::from_bytes(&raw.data, needle)
                        .inspect_err(|_| {
                            crate::log_error!(
                                "server at fd {} core protocol error: malformed message recvd (NewObject)",
                                self.state.stream.as_raw_fd()
                            );
                        })?;

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    self.on_seq(msg.seq(), msg.id());

                    Ok(msg.data().len())
                }
                message::MessageType::GenericProtocolMessage => {
                    let msg = generic_protocol_message::GenericProtocolMessage::from_bytes(
                        &raw.data,
                        &mut raw.fds,
                        needle,
                    )
                    .inspect_err(|e| {
                        match e {
                            message::Error::ArrayTooLong => {
                                trace! { crate::log_debug!("GenericProtocolMessage: failed demarshaling array message, array max size is 10000.") };
                            }
                            message::Error::MalformedMessage => {
                                trace! { crate::log_debug!("[hw] trace: GenericProtocolMessage: failed demarshaling array message") };
                            }
                            _ => {}
                        }
                        crate::log_error!(
                            "server at fd {} core protocol error: malformed message recvd (GenericProtocolMessage)",
                            self.state.stream.as_raw_fd()
                        );
                    })?;

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    let msg_len = msg.data().len();
                    self.on_generic(&msg, dispatch);

                    Ok(msg_len)
                }
                message::MessageType::FatalProtocolError => {
                    let msg = fatal_protocol_error::FatalProtocolError::from_bytes(&raw.data, needle)
                        .inspect_err(|_| {
                            crate::log_error!(
                                "server at fd {} core protocol error: malformed message recvd (FatalProtocolError)",
                                self.state.stream.as_raw_fd()
                            );
                        })?;

                    crate::log_error!(
                        "fatal protocol error: object {} error {}: {}",
                        msg.object_id(),
                        msg.error_id(),
                        msg.error_msg()
                    );
                    self.state.error.store(true, atomic::Ordering::Relaxed);

                    Ok(msg.data().len())
                }
                message::MessageType::RoundtripDone => {
                    let msg = roundtrip_done::RoundtripDone::from_bytes(&raw.data, needle)
                        .inspect_err(|_| {
                            crate::log_error!(
                                "server at fd {} core protocol error: malformed message recvd (RoundtripDone)",
                                self.state.stream.as_raw_fd()
                            );
                        })?;

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    inner.last_ackd_roundtrip_seq = msg.seq();

                    Ok(msg.data().len())
                }
                message::MessageType::BindProtocol
                | message::MessageType::HandshakeAck
                | message::MessageType::RoundtripRequest
                | message::MessageType::Sup => {
                    self.state.error.store(true, atomic::Ordering::Relaxed);
                    crate::log_error!(
                        "server at fd {} core protocol error: invalid message recvd ({message})",
                        self.state.stream.as_raw_fd()
                    );
                    Err(message::Error::InvalidMessage)
                }
            }?;
        }

        if !raw.fds.is_empty() {
            return Err(message::Error::MalformedMessage);
        }

        trace! {
            crate::log_debug!("[hw] trace: [{} @ {}] -- handleMessage: Finished read", self.state.stream.as_raw_fd(), steady_millis())
        }

        Ok(())
    }
}

impl server_client::ServerClientState {
    pub(crate) fn handle_message<D: 'static>(
        &self,
        raw: &mut socket::SocketRawParsedMessage,
        dispatch: &mut D,
    ) -> Result<(), message::Error> {
        let mut needle = 0;
        while needle < raw.data.len() {
            let Ok(message) = message::MessageType::try_from(raw.data[needle]) else {
                crate::log_error!(
                    "client at fd {} core protocol error: invalid message recvd (invalid type code)",
                    self.state.stream.as_raw_fd()
                );
                return Err(message::Error::InvalidMessage);
            };

            needle += match message {
                message::MessageType::Sup => {
                    let msg = hello::Hello::from_bytes(&raw.data, needle).inspect_err(|_| {
                        crate::log_error!(
                            "client at fd {} core protocol error: malformed message recvd (Sup)",
                            self.state.stream.as_raw_fd()
                        );
                    })?;

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    self.dispatch_first_poll();
                    self.state
                        .send_message(&handshake_begin::HandshakeBegin::new(&[1]));

                    Ok(msg.data().len())
                }
                message::MessageType::HandshakeAck => {
                    let msg = handshake_ack::HandshakeAck::from_bytes(&raw.data, needle)
                        .inspect_err(|_| {
                            crate::log_error!(
                                "client at fd {} core protocol error: malformed message recvd (HandshakeAck)",
                                self.state.stream.as_raw_fd()
                            );
                        })?;

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    self.version.store(msg.version(), atomic::Ordering::Relaxed);

                    let protocol_names = self
                        .impls
                        .read()
                        .unwrap()
                        .iter()
                        .map(|imp| {
                            format!(
                                "{}@{}",
                                imp.protocol().spec_name(),
                                imp.protocol().spec_ver()
                            )
                        })
                        .collect::<Vec<_>>();

                    self.state
                        .send_message(&handshake_protocols::HandshakeProtocols::new(
                            &protocol_names,
                        ));

                    Ok(msg.data().len())
                }
                message::MessageType::BindProtocol => {
                    let msg = bind_protocol::BindProtocol::from_bytes(&raw.data, needle)
                        .inspect_err(|_| {
                            crate::log_error!(
                                "client at fd {} core protocol error: malformed message recvd (BindProtocol)",
                                self.state.stream.as_raw_fd()
                            );
                        })?;

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    self.create_object(msg.protocol(), "", msg.version(), msg.seq());

                    Ok(msg.data().len())
                }
                message::MessageType::GenericProtocolMessage => {
                    let msg = generic_protocol_message::GenericProtocolMessage::from_bytes(
                        &raw.data,
                        &mut raw.fds,
                        needle,
                    )
                    .inspect_err(|e| {
                        match e {
                            message::Error::ArrayTooLong => {
                                trace! { crate::log_debug!("GenericProtocolMessage: failed demarshaling array message, array max size is 10000.") };
                            }
                            message::Error::MalformedMessage => {
                                trace! { crate::log_debug!("[hw] trace: GenericProtocolMessage: failed demarshaling array message") };
                            }
                            _ => {}
                        }
                        crate::log_error!(
                            "client at fd {} core protocol error: malformed message recvd (GenericProtocolMessage)",
                            self.state.stream.as_raw_fd()
                        );
                    })?;

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    self.on_generic(&msg, dispatch);

                    Ok(msg.data().len())
                }
                message::MessageType::RoundtripRequest => {
                    let msg = roundtrip_request::RoundtripRequest::from_bytes(&raw.data, needle)
                        .inspect_err(|_| {
                            crate::log_error!(
                                "client at fd {} core protocol error: malformed message recvd (RoundtripRequest)",
                                self.state.stream.as_raw_fd()
                            );
                        })?;

                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }

                    self.scheduled_roundtrip_seq
                        .store(msg.seq(), atomic::Ordering::Relaxed);

                    Ok(msg.data().len())
                }
                message::MessageType::NewObject
                | message::MessageType::HandshakeProtocols
                | message::MessageType::HandshakeBegin
                | message::MessageType::FatalProtocolError
                | message::MessageType::RoundtripDone => {
                    self.state.error.store(true, atomic::Ordering::Relaxed);
                    crate::log_error!(
                        "client at fd {} core protocol error: invalid message recvd ({message})",
                        self.state.stream.as_raw_fd()
                    );
                    Err(message::Error::InvalidMessage)
                }
            }?;
        }

        if !raw.fds.is_empty() {
            return Err(message::Error::MalformedMessage);
        }

        trace! {
            crate::log_debug!("[hw] trace: [{} @ {}] -- handleMessage: Finished read", self.state.stream.as_raw_fd(), steady_millis())
        }

        Ok(())
    }
}
