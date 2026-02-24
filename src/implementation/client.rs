use super::types;

pub struct ObjectImplementation<'a> {
    object_name: &'a str,
    version: u32,
}

pub trait ProtocolImplementations {
    fn protocol(&self) -> &dyn types::ProtocolSpec;

    fn implementation(&self) -> &[ObjectImplementation<'_>];
}
