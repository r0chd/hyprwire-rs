use crate::client::client_socket;
use crate::server::server_client;
use crate::{socket, steady_millis, trace};
use hyprwire_core::message;
use hyprwire_core::message::Message;
use hyprwire_core::message::wire::{
    bind_protocol, fatal_protocol_error, generic_protocol_message, handshake_ack, handshake_begin,
    handshake_protocols, hello, new_object, roundtrip_done, roundtrip_request,
};
use std::os::fd::AsRawFd;

pub enum Role<'a> {
    Client(&'a client_socket::ClientSocket),
    Server(&'a server_client::ServerClientState),
}

impl<'a> Role<'a> {
    fn state(&self) -> &crate::ConnectionState {
        match self {
            Self::Client(client) => &client.state,
            Self::Server(client) => &client.state,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Role::Client(_) => "server",
            Role::Server(_) => "client",
        }
    }
}

pub fn handle_message<D: 'static>(
    raw: &mut socket::SocketRawParsedMessage,
    role: &Role,
    dispatch: &mut D,
) -> Result<(), message::Error> {
    let mut needle = 0;
    while needle < raw.data.len() {
        let Ok(message) = message::MessageType::try_from(raw.data[needle]) else {
            crate::log_error!(
                "server at fd {} core protocol error: invalid message recvd (invalid type code)",
                role.state().stream.as_raw_fd()
            );

            return Err(message::Error::InvalidMessage);
        };

        needle += match (role, message) {
            (Role::Client(client), message::MessageType::HandshakeBegin) => {
                let msg = handshake_begin::HandshakeBegin::from_bytes(&raw.data, needle)
                    .inspect_err(|_| {
                        crate::log_error!(
                            "server at fd {} core protocol error...",
                            client.state.stream.as_raw_fd()
                        );
                    })?;

                if !msg.versions().contains(&crate::PROTOCOL_VERSION) {
                    crate::log_error!(
                        "server at fd {} core protocol error: version negotiation failed",
                        client.state.stream.as_raw_fd()
                    );
                    client.state.error.set(true);
                    return Err(message::Error::VersionNegotiationFailed);
                }

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] -> parse error: {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client
                    .state
                    .send_message(&handshake_ack::HandshakeAck::new(crate::PROTOCOL_VERSION));

                Ok(msg.data().len())
            }
            (Role::Client(client), message::MessageType::HandshakeProtocols) => {
                let msg = handshake_protocols::HandshakeProtocols::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "server at fd {} core protocol error: malformed message recvd (HandshakeProtocols)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.server_specs(msg.protocols());
                client.handshake_done.set(true);

                Ok(msg.data().len())
            }
            (Role::Client(client), message::MessageType::NewObject) => {
                let msg = new_object::NewObject::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "server at fd {} core protocol error: malformed message recvd (NewObject)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.on_seq(msg.seq(), msg.id());

                Ok(msg.data().len())
            }
            (Role::Client(client), message::MessageType::GenericProtocolMessage) => {
                let msg = generic_protocol_message::GenericProtocolMessage::from_bytes(&raw.data, &mut raw.fds, needle)
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
                            client.state.stream.as_raw_fd()
                        );
                    })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                let msg_len = msg.data().len();
                client.on_generic(&msg, dispatch);

                Ok(msg_len)
            }
            (Role::Client(client), message::MessageType::FatalProtocolError) => {
                let msg = fatal_protocol_error::FatalProtocolError::from_bytes(&raw.data, needle)
                    .inspect_err(|_| {
                        crate::log_error!(
                        "server at fd {} core protocol error: malformed message recvd (FatalProtocolError)",
                        client.state.stream.as_raw_fd()
                    );
                    })?;

                crate::log_error!(
                    "fatal protocol error: object {} error {}: {}",
                    msg.object_id(),
                    msg.error_id(),
                    msg.error_msg()
                );
                client.state.error.set(true);

                Ok(msg.data().len())
            }
            (Role::Client(client), message::MessageType::RoundtripDone) => {
                let msg = roundtrip_done::RoundtripDone::from_bytes(&raw.data, needle)
                    .inspect_err(|_| {
                        crate::log_error!(
                        "server at fd {} core protocol error: malformed message recvd (RoundtripDone)",
                        client.state.stream.as_raw_fd()
                    );
                    })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.last_ackd_roundtrip_seq.set(msg.seq());

                Ok(msg.data().len())
            }
            (Role::Server(client), message::MessageType::Sup) => {
                let msg = hello::Hello::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "client at fd {} core protocol error: malformed message recvd (Sup)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.dispatch_first_poll();
                client
                    .state
                    .send_message(&handshake_begin::HandshakeBegin::new(&[1]));

                Ok(msg.data().len())
            }
            (Role::Server(client), message::MessageType::HandshakeAck) => {
                let msg = handshake_ack::HandshakeAck::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "client at fd {} core protocol error: malformed message recvd (HandshakeAck)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.version.set(msg.version());

                let protocol_names = client
                    .state
                    .impls
                    .borrow()
                    .iter()
                    .map(|imp| {
                        format!(
                            "{}@{}",
                            imp.protocol().spec_name(),
                            imp.protocol().spec_ver()
                        )
                    })
                    .collect::<Vec<_>>();

                client
                    .state
                    .send_message(&handshake_protocols::HandshakeProtocols::new(
                        &protocol_names,
                    ));

                Ok(msg.data().len())
            }
            (Role::Server(client), message::MessageType::BindProtocol) => {
                let msg = bind_protocol::BindProtocol::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "client at fd {} core protocol error: malformed message recvd (BindProtocol)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.create_object(msg.protocol(), "", msg.version(), msg.seq());

                Ok(msg.data().len())
            }
            (Role::Server(client), message::MessageType::GenericProtocolMessage) => {
                let msg = generic_protocol_message::GenericProtocolMessage::from_bytes(&raw.data, &mut raw.fds, needle)
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
                            client.state.stream.as_raw_fd()
                        );
                    })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.on_generic(&msg, dispatch);

                Ok(msg.data().len())
            }
            (Role::Server(client), message::MessageType::RoundtripRequest) => {
                let msg = roundtrip_request::RoundtripRequest::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "client at fd {} core protocol error: malformed message recvd (RoundtripRequest)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.scheduled_roundtrip_seq.set(msg.seq());

                Ok(msg.data().len())
            }
            (
                Role::Client(_),
                message::MessageType::BindProtocol
                | message::MessageType::HandshakeAck
                | message::MessageType::RoundtripRequest
                | message::MessageType::Sup,
            )
            | (
                Role::Server(_),
                message::MessageType::NewObject
                | message::MessageType::HandshakeProtocols
                | message::MessageType::HandshakeBegin
                | message::MessageType::FatalProtocolError
                | message::MessageType::RoundtripDone,
            ) => {
                let state = role.state();
                state.error.set(true);

                crate::log_error!(
                    "{} at fd {} core protocol error: invalid message recvd ({message})",
                    role.label(),
                    state.stream.as_raw_fd()
                );

                Err(message::Error::InvalidMessage)
            }
        }?;
    }

    if !raw.fds.is_empty() {
        return Err(message::Error::MalformedMessage);
    }

    trace! {
        crate::log_debug!("[hw] trace: [{} @ {}] -- handleMessage: Finished read", role.state().stream.as_raw_fd(), steady_millis())
    }

    Ok(())
}
