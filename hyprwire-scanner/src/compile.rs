use crate::{generate, parse};
use std::{env, fs, io, path};

pub struct Builder {
    out_dir: Option<path::PathBuf>,
    targets: generate::Targets,
    type_attributes: Vec<(String, String)>,
}

#[must_use]
pub fn configure() -> Builder {
    Builder {
        type_attributes: Vec::new(),
        out_dir: None,
        targets: generate::Targets::ALL,
    }
}

impl Builder {
    #[must_use]
    pub fn out_dir(mut self, path: impl Into<path::PathBuf>) -> Self {
        self.out_dir = Some(path.into());
        self
    }

    #[must_use]
    pub fn with_targets(mut self, targets: generate::Targets) -> Self {
        self.targets = targets;
        self
    }

    /// Add additional attribute to matched enums.
    pub fn type_attribute<P: AsRef<str>, A: AsRef<str>>(mut self, path: P, attribute: A) -> Self {
        self.type_attributes
            .push((path.as_ref().to_string(), attribute.as_ref().to_string()));
        self
    }

    pub fn compile(self, protos: &[impl AsRef<path::Path>]) -> Result<(), io::Error> {
        let out_dir = self
            .out_dir
            .unwrap_or_else(|| path::PathBuf::from(env::var("OUT_DIR").unwrap()));

        let manifest_dir = path::PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

        for proto_path in protos {
            let proto_path = manifest_dir.join(proto_path.as_ref());
            let xml = fs::read_to_string(&proto_path).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("failed to read {}: {e}", proto_path.display()),
                )
            })?;

            let protocol = parse::parse_protocol(&xml).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("failed to parse {}: {e}", proto_path.display()),
                )
            })?;

            let code = generate::generate(&protocol, self.targets, &self.type_attributes);

            let out_name = format!("{}.rs", protocol.name);
            let out_path = out_dir.join(&out_name);
            fs::write(&out_path, code)?;

            println!("cargo::rerun-if-changed={}", proto_path.display());
        }

        Ok(())
    }
}
