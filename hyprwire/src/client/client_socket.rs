use super::{client_object, event_queue};
use crate::client::server_spec;
use crate::implementation::object::Object;
use crate::implementation::wire_object::WireObject;
use crate::{implementation, steady_millis, trace};
use hyprwire_core::message;
use hyprwire_core::message::Message;
use hyprwire_core::message::wire::{bind_protocol, generic_protocol_message, hello};
use hyprwire_core::types::ProtocolSpec;
use polling::AsSource;
use std::os::fd;
use std::os::fd::AsRawFd;
use std::os::unix::net;
use std::sync::atomic;
use std::{ops, path, sync, time};

pub struct ClientSocket {
    pub(crate) poller: polling::Poller,
    impls: sync::RwLock<Vec<Box<dyn implementation::client::ProtocolImplementations>>>,
    server_specs: sync::RwLock<Vec<server_spec::AdvertisedSpec>>,
    objects: sync::RwLock<Vec<sync::Arc<client_object::ClientObject>>>,
    pub(crate) handshake_begin: time::Instant,
    pub(crate) state: sync::Arc<crate::ConnectionState>,
    pub(crate) handshake_done: atomic::AtomicBool,
    pub(crate) seq: atomic::AtomicU32,
    self_ref: sync::Weak<Self>,
}

impl ClientSocket {
    fn new(stream: net::UnixStream) -> crate::Result<sync::Arc<Self>> {
        let poller = polling::Poller::new()?;
        unsafe { poller.add(&stream, polling::Event::readable(0))? };

        let state = sync::Arc::new(crate::ConnectionState::new(stream));

        let client_socket = sync::Arc::new_cyclic(|weak_self| Self {
            poller,
            seq: atomic::AtomicU32::default(),
            impls: sync::RwLock::default(),
            server_specs: sync::RwLock::default(),
            state: sync::Arc::clone(&state),
            objects: sync::RwLock::default(),
            handshake_done: atomic::AtomicBool::default(),
            handshake_begin: time::Instant::now(),
            self_ref: weak_self.clone(),
        });
        state.send_message(&hello::Hello::new());

        Ok(client_socket)
    }

    pub fn connect<P>(path: P) -> crate::Result<sync::Arc<Self>>
    where
        P: AsRef<path::Path>,
    {
        let stream = net::UnixStream::connect(path)?;
        stream.set_nonblocking(true)?;
        Self::new(stream)
    }

    pub fn from_fd<F>(fd: F) -> crate::Result<sync::Arc<Self>>
    where
        F: Into<fd::OwnedFd>,
    {
        let stream = net::UnixStream::from(fd.into());
        stream.set_nonblocking(true)?;
        Self::new(stream)
    }

    pub fn add_implementation(
        &self,
        p_impl: Box<dyn implementation::client::ProtocolImplementations>,
    ) {
        self.impls.write().unwrap().push(p_impl);
    }

    pub fn get_spec(&self, name: &str) -> Option<server_spec::AdvertisedSpec> {
        self.server_specs
            .read()
            .unwrap()
            .iter()
            .find(|spec| spec.name() == name)
            .cloned()
    }

    pub fn bind_protocol(
        &self,
        event_queue: &event_queue::EventQueue,
        spec: &dyn ProtocolSpec,
        version: u32,
    ) -> crate::Result<sync::Arc<client_object::ClientObject>> {
        if version > spec.spec_ver() {
            crate::log_error!(
                "version {} is larger than current spec ver of {}",
                version,
                spec.spec_ver()
            );
            return Err(crate::Error::VersionOutOfRange {
                requested: version,
                max: spec.spec_ver(),
            });
        }

        let mut object = client_object::ClientObject::new(
            self.self_ref.clone(),
            sync::Arc::clone(&self.state),
            event_queue.downgrade(),
        );
        let objects = spec.objects();
        if objects.is_empty() {
            return Err(crate::Error::ProtocolViolation(
                hyprwire_core::message::Error::NoSpec,
            ));
        }
        object.spec = Some(std::sync::Arc::clone(&objects[0]));
        let seq = self.seq.fetch_add(1, atomic::Ordering::Relaxed) + 1;
        object.seq = seq;
        object.version.store(version, atomic::Ordering::Relaxed);
        object.protocol_name = spec.spec_name().to_string();

        let object = sync::Arc::new(object);
        self.objects
            .write()
            .unwrap()
            .push(sync::Arc::clone(&object));

        let bind_message = bind_protocol::BindProtocol::new(spec.spec_name(), seq, version);
        self.state.send_message(&bind_message);

        Ok(object)
    }

    pub(crate) fn wait_for_object<D: 'static>(
        &self,
        event_queue: &event_queue::EventQueue,
        object: &sync::Arc<client_object::ClientObject>,
        dispatch: &mut D,
    ) -> crate::Result<()> {
        while object.id.load(atomic::Ordering::Relaxed) == 0
            && !self.state.error.load(atomic::Ordering::Relaxed)
        {
            event_queue.dispatch_events(dispatch, true)?;
        }

        if self.state.error.load(atomic::Ordering::Relaxed) {
            return Err(crate::Error::ConnectionClosed);
        }

        Ok(())
    }

    pub fn make_object(
        &self,
        protocol_name: &str,
        object_name: &str,
        seq: u32,
        event_queue: &event_queue::EventQueue,
    ) -> Result<sync::Arc<client_object::ClientObject>, message::Error> {
        let mut object = client_object::ClientObject::new(
            self.self_ref.clone(),
            sync::Arc::clone(&self.state),
            event_queue.downgrade(),
        );
        object.protocol_name = protocol_name.to_string();

        if let Some(obj) = self
            .impls
            .read()
            .unwrap()
            .iter()
            .find(|imp| imp.protocol().spec_name() == protocol_name)
            .and_then(|imp| {
                imp.protocol()
                    .objects()
                    .iter()
                    .find(|obj| obj.object_name() == object_name)
            })
        {
            object.spec = Some(std::sync::Arc::clone(obj));
        }

        if object.spec.is_none() {
            return Err(message::Error::NoSpec);
        }

        object.seq = seq;
        object.set_version(0); // TODO: client version doesn't matter that much, but for verification's sake we could fix this

        let object = sync::Arc::new(object);
        self.objects
            .write()
            .unwrap()
            .push(sync::Arc::clone(&object));
        Ok(object)
    }

    pub fn extract_loop_fd(&self) -> fd::BorrowedFd<'_> {
        self.poller.source()
    }

    pub fn server_specs(&self, specs: &[Box<str>]) {
        let mut server_specs = self.server_specs.write().unwrap();
        for spec in specs {
            let at_pos = spec.rfind('@').unwrap();

            let s = server_spec::AdvertisedSpec::new(
                spec[0..at_pos].to_string(),
                spec[at_pos + 1..].parse().unwrap(),
            );
            server_specs.push(s);
        }
    }

    pub fn disconnect_on_error(&self) {
        self.state.error.store(true, atomic::Ordering::Relaxed);
        let _ = self.state.stream.shutdown(std::net::Shutdown::Both);
    }

    pub fn on_seq(&self, seq: u32, id: u32) {
        let objects = self.objects.read().unwrap();
        if let Some(object) = objects.iter().find(|object| object.seq == seq) {
            object.id.store(id, atomic::Ordering::Relaxed);
        }
    }

    pub fn destroy_object(&self, id: u32) {
        self.objects
            .write()
            .unwrap()
            .retain(|obj| obj.id.load(atomic::Ordering::Relaxed) != id);
    }

    pub fn collect_orphaned_objects(&self) {
        self.objects.write().unwrap().retain(|obj| {
            if obj.id.load(atomic::Ordering::Relaxed) == 0 {
                return true;
            }

            let should_remove = sync::Arc::strong_count(obj) <= 1;

            if should_remove {
                trace! {
                    crate::log_debug!("[{} @ {:.3}] -> Cleaning up orphaned object {}", self.state.stream.as_raw_fd(), steady_millis(), obj.id.load(atomic::Ordering::Relaxed))
                }
            }

            !should_remove
        });
    }

    pub fn on_generic<D: 'static>(
        &self,
        msg: &generic_protocol_message::GenericProtocolMessage<ops::Range<usize>>,
        dispatch: &mut D,
    ) {
        let obj = self
            .objects
            .read()
            .unwrap()
            .iter()
            .find(|obj| obj.id.load(atomic::Ordering::Relaxed) == msg.object())
            .map(sync::Arc::clone);

        if let Some(obj) = obj {
            obj.dispatch(msg.method(), msg.data_span(), msg.fds(), dispatch);

            // Handle destructor methods
            if let Some(spec) = &obj.spec
                && let Some(method) = spec.s2c().get(msg.method() as usize)
                && method.destructor
            {
                obj.destroyed.store(true, atomic::Ordering::Relaxed);
                let id = obj.id.load(atomic::Ordering::Relaxed);
                if id != 0 {
                    self.destroy_object(id);
                }
            }
        } else {
            crate::log_error!(
                "[{} @ {:.3}] generic message references unknown object {}",
                self.state.stream.as_raw_fd(),
                steady_millis(),
                msg.object(),
            );
            self.disconnect_on_error();
        }
    }

    pub fn object_for_id(&self, id: u32) -> Option<sync::Arc<client_object::ClientObject>> {
        self.objects
            .read()
            .unwrap()
            .iter()
            .find(|object| object.id.load(atomic::Ordering::Relaxed) == id)
            .map(sync::Arc::clone)
    }

    pub fn object_for_seq(&self, seq: u32) -> Option<sync::Arc<client_object::ClientObject>> {
        self.objects
            .read()
            .unwrap()
            .iter()
            .find(|object| object.seq == seq)
            .map(sync::Arc::clone)
    }
}
