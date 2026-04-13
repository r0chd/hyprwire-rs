use crate::implementation::{client, types};
use std::marker;

#[derive(Clone)]
pub(crate) struct AdvertisedSpec {
    name: String,
    version: u32,
}

impl AdvertisedSpec {
    pub(crate) fn new(name: String, version: u32) -> Self {
        Self { name, version }
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn version(&self) -> u32 {
        self.version
    }
}

pub struct ServerSpec<I> {
    version: u32,
    protocol: Box<dyn types::ProtocolSpec>,
    _impl: marker::PhantomData<I>,
}

impl<I> ServerSpec<I>
where
    I: client::ProtocolImplementations,
{
    pub(crate) fn new(version: u32) -> Self {
        Self {
            version,
            protocol: I::protocol_spec(),
            _impl: marker::PhantomData,
        }
    }
}

impl<I> Clone for ServerSpec<I>
where
    I: client::ProtocolImplementations,
{
    fn clone(&self) -> Self {
        Self::new(self.version)
    }
}

impl<I> types::ProtocolSpec for ServerSpec<I>
where
    I: client::ProtocolImplementations,
{
    fn spec_name(&self) -> &str {
        self.protocol.spec_name()
    }

    fn spec_ver(&self) -> u32 {
        self.version
    }

    fn objects(&self) -> &[std::sync::Arc<dyn types::ProtocolObjectSpec>] {
        self.protocol.objects()
    }
}
