use super::object;
use hyprwire_core::types;
use std::rc;

type OnBind<'a> = Box<dyn Fn(rc::Rc<dyn object::Object>) + 'a>;

pub struct ObjectImplementation<'a> {
    pub object_name: &'a str,
    pub version: u32,
    pub on_bind: OnBind<'a>,
}

pub trait ProtocolImplementations {
    fn protocol(&self) -> &dyn types::ProtocolSpec;

    fn implementation(&self) -> &[ObjectImplementation<'_>];
}

pub trait Construct<H>: ProtocolImplementations {
    fn new(version: u32, handler: &mut H) -> Self;
}
