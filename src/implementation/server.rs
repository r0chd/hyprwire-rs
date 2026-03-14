use super::{object, types};
use std::{cell, rc};

pub struct ObjectImplementation<'a> {
    pub object_name: &'a str,
    pub version: u32,
}

pub trait ProtocolImplementations {
    fn protocol(&self) -> &dyn types::ProtocolSpec;

    fn implementation(&self) -> &[ObjectImplementation<'_>];

    fn on_bind(&self, _obj: rc::Rc<cell::RefCell<dyn object::Object>>) {}
}
