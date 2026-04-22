use hyprwire_core::types;

#[allow(dead_code)]
pub struct ObjectImplementation<'a> {
    pub object_name: &'a str,
    pub version: u32,
}

pub trait ProtocolImplementations {
    fn new() -> Self
    where
        Self: Sized;

    fn protocol_spec() -> Box<dyn types::ProtocolSpec>
    where
        Self: Sized;

    fn spec_name() -> &'static str
    where
        Self: Sized;

    fn protocol(&self) -> &dyn types::ProtocolSpec;

    fn implementation(&self) -> &[ObjectImplementation<'_>];
}
