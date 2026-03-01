use crate::implementation::types;

#[derive(Clone)]
pub struct ServerSpec {
    name: String,
    version: u32,
}

impl ServerSpec {
    pub fn new(name: String, version: u32) -> Self {
        Self { name, version }
    }
}

impl types::ProtocolSpec for ServerSpec {
    fn spec_name(&self) -> &str {
        &self.name
    }

    fn spec_ver(&self) -> u32 {
        self.version
    }

    fn objects(&self) -> &[&dyn types::ProtocolObjectSpec] {
        &[]
    }
}
