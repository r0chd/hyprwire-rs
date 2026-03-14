use super::{object, types};
use std::{cell, rc};

pub struct ObjectImplementation<'a> {
    pub object_name: &'a str,
    pub version: u32,
    pub on_bind: Box<dyn Fn(rc::Rc<cell::RefCell<dyn object::Object>>) + 'a>,
}

pub trait ProtocolImplementations {
    fn protocol(&self) -> &dyn types::ProtocolSpec;

    fn implementation(&self) -> &[ObjectImplementation<'_>];
}
