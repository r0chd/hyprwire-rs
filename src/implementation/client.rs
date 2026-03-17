use super::types;

#[allow(dead_code)]
pub struct ObjectImplementation<'a> {
    pub object_name: &'a str,
    pub version: u32,
}

pub trait ProtocolImplementations {
    fn protocol(&self) -> &dyn types::ProtocolSpec;

    fn implementation(&self) -> &[ObjectImplementation<'_>];
}
