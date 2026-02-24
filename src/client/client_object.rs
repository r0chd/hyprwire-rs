use crate::implementation::{object, types, wire_object};
use crate::{client, message};
use std::os::raw;

pub struct ClientObject<'a> {
    client: Option<client::ClientSocket<'a>>,
    spec: Option<&'a dyn types::ProtocolObjectSpec>,
    data: Option<*mut raw::c_void>,
    listeners: Vec<*mut raw::c_void>,
    pub(crate) id: u32,
    version: u32,
    pub(crate) seq: u32,
    protocol_name: String,
}

impl object::Object for ClientObject<'_> {
    fn client_sock(&self) -> Option<&client::ClientSocket<'_>> {
        self.client.as_ref()
    }

    fn error(&self, error_id: u32, error_msg: &str) {
        _ = error_id;
        _ = error_msg;
    }
}

impl wire_object::WireObject for ClientObject<'_> {
    fn version(&self) -> u32 {
        self.version
    }

    fn id(&self) -> u32 {
        self.id
    }

    fn server(&self) -> bool {
        false
    }

    fn methods_out(&self) -> &[types::Method] {
        self.spec.map(|spec| spec.c2s()).unwrap_or_default()
    }

    fn methods_in(&self) -> &[types::Method] {
        self.spec.map(|spec| spec.s2c()).unwrap_or_default()
    }

    fn errd(&mut self) {
        if let Some(client) = self.client.as_mut() {
            client.error = true;
        }
    }

    fn send_message(&mut self, msg: &dyn message::Message) {
        if let Some(client) = self.client.as_mut() {
            client.send_message(msg);
        }
    }

    fn listeners(&self) -> &[*mut std::os::raw::c_void] {
        &self.listeners
    }
}
