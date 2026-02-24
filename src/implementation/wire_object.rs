use super::types;
use crate::implementation::object;
use crate::message;

pub trait WireObject: object::Object {
    fn version(&self) -> u32;

    // fn listeners(&self) -> &[anyopaque];

    fn methods_out(&self) -> &[types::Method];

    fn methods_in(&self) -> &[types::Method];

    fn errd(&mut self);

    fn send_message(&mut self, msg: &dyn message::Message);

    fn server(&self) -> bool;

    fn get_id(&self) -> u32;

    fn called(&self, id: u32, _data: &[u8], _fds: &[i32]) {
        let methods = self.methods_in();

        if methods.len() <= id as usize {
            let msg = format!("invalid method {} for object {}", id, self.get_id());
            log::debug!("core protocol error: {msg}");
            self.error(self.get_id(), &msg);
            // return InvalidMethod error
        }

        // if self.listeners().len() <= id {}
    }
}
