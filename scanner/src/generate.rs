use super::parse::{ArgType, Method, Protocol};
use std::fmt::Write;

fn snake_to_pascal(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

fn snake_to_screaming(s: &str) -> String {
    s.to_uppercase()
}

struct W {
    buf: String,
    indent: usize,
}

impl W {
    fn new() -> Self {
        Self {
            buf: String::new(),
            indent: 0,
        }
    }

    fn line(&mut self, s: &str) {
        if s.is_empty() {
            self.buf.push('\n');
            return;
        }
        for _ in 0..self.indent {
            self.buf.push_str("    ");
        }
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    fn indent(&mut self) {
        self.indent += 1;
    }

    fn dedent(&mut self) {
        self.indent -= 1;
    }
}

fn magic_for_arg(arg_type: &ArgType) -> Vec<&'static str> {
    match arg_type {
        ArgType::Varchar => vec!["TypeVarchar"],
        ArgType::Fd => vec!["TypeFd"],
        ArgType::Uint | ArgType::Enum => vec!["TypeUint"],
        ArgType::Int => vec!["TypeInt"],
        ArgType::F32 => vec!["TypeF32"],
        ArgType::ArrayVarchar => vec!["TypeArray", "TypeVarchar"],
        ArgType::ArrayFd => vec!["TypeArray", "TypeFd"],
        ArgType::ArrayUint => vec!["TypeArray", "TypeUint"],
        ArgType::ArrayInt => vec!["TypeArray", "TypeInt"],
        ArgType::ArrayF32 => vec!["TypeArray", "TypeF32"],
    }
}

fn event_field_type(arg_type: &ArgType) -> &'static str {
    match arg_type {
        ArgType::Varchar => "&'a ffi::CStr",
        ArgType::Fd => "i32",
        ArgType::Uint | ArgType::Enum => "u32",
        ArgType::Int => "i32",
        ArgType::F32 => "f32",
        ArgType::ArrayVarchar => "&'a [&'a ffi::CStr]",
        ArgType::ArrayFd => "&'a [i32]",
        ArgType::ArrayUint => "&'a [u32]",
        ArgType::ArrayInt => "&'a [i32]",
        ArgType::ArrayF32 => "&'a [f32]",
    }
}

fn dispatch_param_type(arg_type: &ArgType) -> &'static str {
    match arg_type {
        ArgType::Varchar => "*const ffi::c_char",
        ArgType::Fd | ArgType::Int => "i32",
        ArgType::Uint | ArgType::Enum => "u32",
        ArgType::F32 => "f32",
        ArgType::ArrayVarchar => "*const *const ffi::c_char",
        ArgType::ArrayFd | ArgType::ArrayInt => "*const i32",
        ArgType::ArrayUint => "*const u32",
        ArgType::ArrayF32 => "*const f32",
    }
}

fn is_array_type(arg_type: &ArgType) -> bool {
    matches!(
        arg_type,
        ArgType::ArrayVarchar
            | ArgType::ArrayFd
            | ArgType::ArrayUint
            | ArgType::ArrayInt
            | ArgType::ArrayF32
    )
}

fn send_param_type(arg_type: &ArgType, interface: Option<&str>) -> String {
    match arg_type {
        ArgType::Varchar => "&str".to_string(),
        ArgType::Fd => "i32".to_string(),
        ArgType::Uint => "u32".to_string(),
        ArgType::Int => "i32".to_string(),
        ArgType::F32 => "f32".to_string(),
        ArgType::Enum => format!("super::spec::{}", snake_to_pascal(interface.unwrap())),
        ArgType::ArrayVarchar => "&[&str]".to_string(),
        ArgType::ArrayFd => "&[i32]".to_string(),
        ArgType::ArrayUint => "&[u32]".to_string(),
        ArgType::ArrayInt => "&[i32]".to_string(),
        ArgType::ArrayF32 => "&[f32]".to_string(),
    }
}

fn call_arg_expr(name: &str, arg_type: &ArgType) -> String {
    match arg_type {
        ArgType::Varchar => {
            format!("hyprwire::implementation::types::CallArg::Varchar({name}.as_bytes())")
        }
        ArgType::Fd => format!("hyprwire::implementation::types::CallArg::Fd({name})"),
        ArgType::Uint => format!("hyprwire::implementation::types::CallArg::Uint({name})"),
        ArgType::Int => format!("hyprwire::implementation::types::CallArg::Int({name})"),
        ArgType::F32 => format!("hyprwire::implementation::types::CallArg::F32({name})"),
        ArgType::Enum => format!("hyprwire::implementation::types::CallArg::Uint({name} as u32)"),
        ArgType::ArrayVarchar => {
            "hyprwire::implementation::types::CallArg::VarcharArray(&bytes)".to_string()
        }
        ArgType::ArrayFd => {
            format!("hyprwire::implementation::types::CallArg::FdArray({name})")
        }
        ArgType::ArrayUint => {
            format!("hyprwire::implementation::types::CallArg::UintArray({name})")
        }
        ArgType::ArrayInt => {
            format!("hyprwire::implementation::types::CallArg::IntArray({name})")
        }
        ArgType::ArrayF32 => {
            format!("hyprwire::implementation::types::CallArg::F32Array({name})")
        }
    }
}

// --- Spec module ---

fn generate_spec(w: &mut W, protocol: &Protocol) {
    w.line("pub mod spec {");
    w.indent();

    // Enums
    for e in &protocol.enums {
        let pascal = snake_to_pascal(&e.name);
        w.line("#[repr(u32)]");
        w.line(&format!("pub enum {pascal} {{"));
        w.indent();
        for v in &e.values {
            w.line(&format!("{} = {},", snake_to_pascal(&v.name), v.idx));
        }
        w.dedent();
        w.line("}");
        w.line("");
    }

    // Object specs
    for obj in &protocol.objects {
        let pascal = snake_to_pascal(&obj.name);
        let screaming = snake_to_screaming(&obj.name);
        let spec_name = format!("{pascal}Spec");

        w.line(&format!("pub struct {spec_name} {{"));
        w.indent();
        w.line("c2s_methods: &'static [hyprwire::implementation::types::Method],");
        w.line("s2c_methods: &'static [hyprwire::implementation::types::Method],");
        w.dedent();
        w.line("}");
        w.line("");

        w.line(&format!("static {screaming}: {spec_name} = {spec_name} {{"));
        w.indent();

        // c2s methods
        w.line("c2s_methods: &[");
        w.indent();
        for (idx, m) in obj.c2s.iter().enumerate() {
            write_method_spec(w, idx, m);
        }
        w.dedent();
        w.line("],");

        // s2c methods
        w.line("s2c_methods: &[");
        w.indent();
        for (idx, m) in obj.s2c.iter().enumerate() {
            write_method_spec(w, idx, m);
        }
        w.dedent();
        w.line("],");

        w.dedent();
        w.line("};");
        w.line("");

        // ProtocolObjectSpec impl
        w.line(&format!(
            "impl hyprwire::implementation::types::ProtocolObjectSpec for {spec_name} {{"
        ));
        w.indent();
        w.line(&format!(
            "fn object_name(&self) -> &str {{ \"{}\" }}",
            obj.name
        ));
        w.line("");
        w.line("fn c2s(&self) -> &[hyprwire::implementation::types::Method] { self.c2s_methods }");
        w.line("");
        w.line("fn s2c(&self) -> &[hyprwire::implementation::types::Method] { self.s2c_methods }");
        w.dedent();
        w.line("}");
        w.line("");
    }

    // ProtocolSpec struct
    let proto_pascal = snake_to_pascal(&protocol.name);
    let proto_spec = format!("{proto_pascal}ProtocolSpec");
    let num_objects = protocol.objects.len();

    w.line("#[derive(Copy, Clone)]");
    w.line(&format!("pub struct {proto_spec} {{"));
    w.indent();
    w.line(&format!(
        "objects: [&'static dyn hyprwire::implementation::types::ProtocolObjectSpec; {num_objects}],"
    ));
    w.dedent();
    w.line("}");
    w.line("");

    // Default impl
    w.line(&format!("impl Default for {proto_spec} {{"));
    w.indent();
    w.line("fn default() -> Self {");
    w.indent();
    w.line("Self {");
    w.indent();
    let obj_refs: Vec<String> = protocol
        .objects
        .iter()
        .map(|o| format!("&{}", snake_to_screaming(&o.name)))
        .collect();
    w.line(&format!("objects: [{}],", obj_refs.join(", ")));
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.line("");

    // ProtocolSpec impl
    w.line(&format!(
        "impl hyprwire::implementation::types::ProtocolSpec for {proto_spec} {{"
    ));
    w.indent();
    w.line(&format!(
        "fn spec_name(&self) -> &str {{ \"{}\" }}",
        protocol.name
    ));
    w.line("");
    w.line(&format!(
        "fn spec_ver(&self) -> u32 {{ {} }}",
        protocol.version
    ));
    w.line("");
    w.line(
        "fn objects(&self) -> &[&dyn hyprwire::implementation::types::ProtocolObjectSpec] { &self.objects }",
    );
    w.dedent();
    w.line("}");

    w.dedent();
    w.line("}");
}

fn write_method_spec(w: &mut W, idx: usize, m: &Method) {
    w.line("hyprwire::implementation::types::Method {");
    w.indent();
    w.line(&format!("idx: {idx},"));

    // params
    let mut params = Vec::new();
    for arg in &m.args {
        for magic in magic_for_arg(&arg.arg_type) {
            params.push(format!(
                "hyprwire::implementation::types::MessageMagic::{magic} as u8"
            ));
        }
    }
    if params.is_empty() {
        w.line("params: &[],");
    } else if params.len() == 1 {
        w.line(&format!("params: &[{}],", params[0]));
    } else {
        w.line("params: &[");
        w.indent();
        for p in &params {
            w.line(&format!("{p},"));
        }
        w.dedent();
        w.line("],");
    }

    let ret = m.returns.as_deref().unwrap_or("");
    w.line(&format!("returns_type: \"{ret}\","));
    w.line("since: 0,");
    w.dedent();
    w.line("},");
}

// --- Server module ---

fn generate_server(w: &mut W, protocol: &Protocol) {
    w.line("pub mod server {");
    w.indent();
    w.line("use std::{cell, ffi, rc};");
    w.line("");

    // Structs for all objects
    for obj in &protocol.objects {
        let pascal = snake_to_pascal(&obj.name);
        w.line(&format!("pub struct {pascal}Object {{"));
        w.indent();
        w.line("object: hyprwire::implementation::types::Object,");
        w.dedent();
        w.line("}");
        w.line("");
    }

    // For each object: event enum, proxy impl, dispatch functions, impl block
    for obj in &protocol.objects {
        let pascal = snake_to_pascal(&obj.name);
        let obj_type = format!("{pascal}Object");
        let event_type = format!("{pascal}Event");

        // Event enum for c2s methods (what the server receives)
        if !obj.c2s.is_empty() {
            write_event_enum(w, &event_type, &obj.c2s);

            // Proxy impl
            w.line(&format!("impl hyprwire::Proxy for {obj_type} {{"));
            w.indent();
            w.line(&format!("type Event<'a> = {event_type}<'a>;"));
            w.dedent();
            w.line("}");
            w.line("");

            // Dispatch functions for c2s
            for (idx, m) in obj.c2s.iter().enumerate() {
                write_dispatch_fn(w, &obj.name, &obj_type, &event_type, idx, m, false);
            }
        }

        // Impl block with new + send methods
        w.line(&format!("impl {obj_type} {{"));
        w.indent();

        // new function
        w.line("pub fn new<D: hyprwire::Dispatch<Self>>(");
        w.indent();
        w.line("object: hyprwire::implementation::types::Object,");
        w.dedent();
        w.line(") -> Self {");
        w.indent();
        w.line("unsafe fn drop_dispatch_data(ptr: *mut ffi::c_void) {");
        w.indent();
        w.line("drop(unsafe { Box::from_raw(ptr as *mut hyprwire::DispatchData) });");
        w.dedent();
        w.line("}");
        w.line("");
        w.line("let dispatch_data = Box::into_raw(Box::new(hyprwire::DispatchData {");
        w.indent();
        w.line("object: rc::Rc::as_ptr(object.inner()),");
        w.dedent();
        w.line("}));");
        w.line("");
        w.line("{");
        w.indent();
        w.line("let mut obj = object.inner().borrow_mut();");
        w.line("obj.set_data(dispatch_data as *mut ffi::c_void, Some(drop_dispatch_data));");
        for (idx, _m) in obj.c2s.iter().enumerate() {
            w.line(&format!(
                "obj.listen({idx}, {}_method{idx}::<D> as *mut ffi::c_void);",
                obj.name
            ));
        }
        w.dedent();
        w.line("}");
        w.line("");
        w.line("Self { object }");
        w.dedent();
        w.line("}");
        w.line("");
        w.line("pub fn error(&self, error_id: u32, error_msg: &str) {");
        w.indent();
        w.line("self.object.inner().borrow().error(error_id, error_msg);");
        w.dedent();
        w.line("}");

        // Send methods for s2c (what the server sends to clients)
        for (idx, m) in obj.s2c.iter().enumerate() {
            w.line("");
            write_send_method(w, idx, m);
        }

        w.dedent();
        w.line("}");
        w.line("");
    }

    // Handler trait
    let proto_pascal = snake_to_pascal(&protocol.name);
    let handler_name = format!("{proto_pascal}Handler");
    let impl_name = format!("{proto_pascal}Impl");

    w.line(&format!("pub trait {handler_name} {{"));
    w.indent();
    w.line("fn bind(&mut self, object: hyprwire::implementation::types::Object);");
    w.dedent();
    w.line("}");
    w.line("");

    // Server protocol Impl struct
    w.line(&format!("pub struct {impl_name} {{"));
    w.indent();
    w.line("version: u32,");
    w.line(&format!("handler: *mut dyn {handler_name},"));
    w.line(&format!(
        "protocol: super::spec::{proto_pascal}ProtocolSpec,"
    ));
    w.line("impls: Vec<hyprwire::implementation::server::ObjectImplementation<'static>>,");
    w.dedent();
    w.line("}");
    w.line("");

    w.line(&format!("impl {impl_name} {{"));
    w.indent();
    w.line(&format!(
        "pub fn new(version: u32, handler: &mut (impl {handler_name} + 'static)) -> Self {{"
    ));
    w.indent();
    w.line(&format!(
        "let handler = handler as *mut dyn {handler_name};"
    ));
    w.line("Self {");
    w.indent();
    w.line("version,");
    w.line("handler,");
    w.line(&format!(
        "protocol: super::spec::{proto_pascal}ProtocolSpec::default(),"
    ));
    w.line("impls: vec![");
    w.indent();
    for (idx, obj) in protocol.objects.iter().enumerate() {
        w.line("hyprwire::implementation::server::ObjectImplementation {");
        w.indent();
        w.line(&format!("object_name: \"{}\",", obj.name));
        w.line("version,");
        if idx == 0 {
            w.line("on_bind: Box::new(move |obj| {");
            w.indent();
            w.line("let object = hyprwire::implementation::types::Object::from_raw(obj);");
            w.line("unsafe { &mut *handler }.bind(object);");
            w.dedent();
            w.line("}),");
        } else {
            w.line("on_bind: Box::new(|_obj| {}),");
        }
        w.dedent();
        w.line("},");
    }
    w.dedent();
    w.line("],");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.line("");

    // ProtocolImplementations impl
    w.line(&format!(
        "impl hyprwire::implementation::server::ProtocolImplementations for {impl_name} {{"
    ));
    w.indent();
    w.line("fn protocol(&self) -> &dyn hyprwire::implementation::types::ProtocolSpec {");
    w.indent();
    w.line("&self.protocol");
    w.dedent();
    w.line("}");
    w.line("");
    w.line(
        "fn implementation(&self) -> &[hyprwire::implementation::server::ObjectImplementation<'_>] {",
    );
    w.indent();
    w.line("&self.impls");
    w.dedent();
    w.line("}");

    w.dedent();
    w.line("}");

    w.dedent();
    w.line("}");
}

// --- Client module ---

fn generate_client(w: &mut W, protocol: &Protocol) {
    w.line("pub mod client {");
    w.indent();
    w.line("use std::{cell, ffi, rc};");
    w.line("");

    // Structs for all objects
    for obj in &protocol.objects {
        let pascal = snake_to_pascal(&obj.name);
        w.line(&format!("pub struct {pascal}Object {{"));
        w.indent();
        w.line("object: hyprwire::implementation::types::Object,");
        w.line("on_destroy: Option<Box<dyn FnOnce()>>,");
        w.dedent();
        w.line("}");
        w.line("");
    }

    // For each object: event enum, proxy impl, dispatch functions, impl block
    for obj in &protocol.objects {
        let pascal = snake_to_pascal(&obj.name);
        let obj_type = format!("{pascal}Object");
        let event_type = format!("{pascal}Event");

        // Event enum (s2c methods)
        if !obj.s2c.is_empty() {
            write_event_enum(w, &event_type, &obj.s2c);

            // Proxy impl
            w.line(&format!("impl hyprwire::Proxy for {obj_type} {{"));
            w.indent();
            w.line(&format!("type Event<'a> = {event_type}<'a>;"));
            w.dedent();
            w.line("}");
            w.line("");

            // Dispatch functions for s2c
            for (idx, m) in obj.s2c.iter().enumerate() {
                write_dispatch_fn(w, &obj.name, &obj_type, &event_type, idx, m, true);
            }
        }

        // Impl block with new + send methods
        w.line(&format!("impl {obj_type} {{"));
        w.indent();

        // new function
        w.line("pub fn new<D: hyprwire::Dispatch<Self>>(");
        w.indent();
        w.line("object: hyprwire::implementation::types::Object,");
        w.dedent();
        w.line(") -> Self {");
        w.indent();
        w.line("unsafe fn drop_dispatch_data(ptr: *mut ffi::c_void) {");
        w.indent();
        w.line("drop(unsafe { Box::from_raw(ptr as *mut hyprwire::DispatchData) });");
        w.dedent();
        w.line("}");
        w.line("");
        w.line("let dispatch_data = Box::into_raw(Box::new(hyprwire::DispatchData {");
        w.indent();
        w.line("object: rc::Rc::as_ptr(object.inner()),");
        w.dedent();
        w.line("}));");
        w.line("");
        w.line("{");
        w.indent();
        w.line("let mut obj = object.inner().borrow_mut();");
        w.line("obj.set_data(dispatch_data as *mut ffi::c_void, Some(drop_dispatch_data));");
        for (idx, _m) in obj.s2c.iter().enumerate() {
            w.line(&format!(
                "obj.listen({idx}, {}_method{idx}::<D> as *mut ffi::c_void);",
                obj.name
            ));
        }
        w.dedent();
        w.line("}");
        w.line("");
        w.line("Self { object, on_destroy: None }");
        w.dedent();
        w.line("}");

        // set_on_destroy
        w.line("");
        w.line("pub fn set_on_destroy(&mut self, callback: impl FnOnce() + 'static) {");
        w.indent();
        w.line("self.on_destroy = Some(Box::new(callback));");
        w.dedent();
        w.line("}");

        // Send methods for c2s
        for (idx, m) in obj.c2s.iter().enumerate() {
            w.line("");
            write_send_method(w, idx, m);
        }

        w.dedent();
        w.line("}");
        w.line("");

        // Drop impl
        w.line(&format!("impl Drop for {obj_type} {{"));
        w.indent();
        w.line("fn drop(&mut self) {");
        w.indent();
        w.line("if let Some(cb) = self.on_destroy.take() {");
        w.indent();
        w.line("cb();");
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
        w.line("");
    }

    // Protocol impl struct
    let proto_pascal = snake_to_pascal(&protocol.name);
    w.line("#[derive(Default, Copy, Clone)]");
    w.line(&format!("pub struct {proto_pascal}Impl {{"));
    w.indent();
    w.line(&format!(
        "protocol: super::spec::{proto_pascal}ProtocolSpec,"
    ));
    w.dedent();
    w.line("}");
    w.line("");

    w.line(&format!(
        "impl hyprwire::implementation::client::ProtocolImplementations for {proto_pascal}Impl {{"
    ));
    w.indent();
    w.line("fn protocol(&self) -> &dyn hyprwire::implementation::types::ProtocolSpec {");
    w.indent();
    w.line("&self.protocol");
    w.dedent();
    w.line("}");
    w.line("");
    w.line(
        "fn implementation(&self) -> &[hyprwire::implementation::client::ObjectImplementation<'_>] {",
    );
    w.indent();
    w.line("&[]");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");

    w.dedent();
    w.line("}");
}

// --- Shared helpers ---

fn write_event_enum(w: &mut W, event_type: &str, methods: &[Method]) {
    w.line(&format!("pub enum {event_type}<'a> {{"));
    w.indent();
    for m in methods {
        let variant = snake_to_pascal(&m.name);
        if m.args.is_empty() && m.returns.is_some() {
            w.line(&format!("{variant} {{ seq: u32 }},"));
        } else if m.args.is_empty() {
            w.line(&format!("{variant},"));
        } else {
            let fields: Vec<String> = m
                .args
                .iter()
                .map(|a| format!("{}: {}", a.name, event_field_type(&a.arg_type)))
                .collect();
            w.line(&format!("{variant} {{ {} }},", fields.join(", ")));
        }
    }
    w.dedent();
    w.line("}");
    w.line("");
}

fn write_dispatch_fn(
    w: &mut W,
    obj_name: &str,
    obj_type: &str,
    event_type: &str,
    idx: usize,
    m: &Method,
    has_on_destroy: bool,
) {
    // Function signature
    let fn_name = format!("{obj_name}_method{idx}");
    let mut params = vec!["data: *mut ffi::c_void".to_string()];

    if m.args.is_empty() && m.returns.is_some() {
        params.push("seq: u32".to_string());
    } else {
        for arg in &m.args {
            params.push(format!(
                "{}: {}",
                arg.name,
                dispatch_param_type(&arg.arg_type)
            ));
            if is_array_type(&arg.arg_type) {
                params.push(format!("{}_len: u32", arg.name));
            }
        }
    }

    w.line(&format!(
        "unsafe extern \"C\" fn {fn_name}<D: hyprwire::Dispatch<{obj_type}>>(",
    ));
    w.indent();
    for p in &params {
        w.line(&format!("{p},"));
    }
    w.dedent();
    w.line(") {");
    w.indent();

    // Body: standard preamble
    w.line("let dispatch = unsafe { &*(data as *const hyprwire::DispatchData) };");
    w.line("let state = unsafe { &mut *(hyprwire::get_dispatch_state() as *mut D) };");
    w.line("unsafe { rc::Rc::increment_strong_count(dispatch.object) };");
    w.line(&format!("let proxy = {obj_type} {{"));
    w.indent();
    w.line("object: hyprwire::implementation::types::Object::from_raw(");
    w.indent();
    w.line("unsafe { rc::Rc::from_raw(dispatch.object) },");
    w.dedent();
    w.line("),");
    if has_on_destroy {
        w.line("on_destroy: None,");
    }
    w.dedent();
    w.line("};");

    // Arg conversions
    let variant = snake_to_pascal(&m.name);
    let mut event_fields = Vec::new();

    if m.args.is_empty() && m.returns.is_some() {
        event_fields.push("seq".to_string());
    } else {
        for arg in &m.args {
            match &arg.arg_type {
                ArgType::Varchar => {
                    w.line(&format!(
                        "let {} = unsafe {{ ffi::CStr::from_ptr({}) }};",
                        arg.name, arg.name
                    ));
                    event_fields.push(arg.name.clone());
                }
                ArgType::ArrayVarchar => {
                    w.line(&format!(
                        "let ptrs = unsafe {{ std::slice::from_raw_parts({}, {}_len as usize) }};",
                        arg.name, arg.name
                    ));
                    w.line("let strings: Vec<&ffi::CStr> = ptrs");
                    w.indent();
                    w.line(".iter()");
                    w.line(".map(|&p| unsafe { ffi::CStr::from_ptr(p) })");
                    w.line(".collect();");
                    w.dedent();
                    event_fields.push(format!("{}: &strings", arg.name));
                }
                t if is_array_type(t) => {
                    w.line(&format!(
                        "let {} = unsafe {{ std::slice::from_raw_parts({}, {}_len as usize) }};",
                        arg.name, arg.name, arg.name
                    ));
                    event_fields.push(arg.name.clone());
                }
                _ => {
                    // fd, uint, int, f32, enum - no conversion needed
                    event_fields.push(arg.name.clone());
                }
            }
        }
    }

    // state.event call
    let fields_str = event_fields.join(", ");
    if event_fields.is_empty() {
        w.line(&format!("state.event(&proxy, {event_type}::{variant});"));
    } else if event_fields.iter().any(|f| f.contains(':')) {
        // Has renamed fields like "message: &strings"
        w.line("state.event(");
        w.indent();
        w.line("&proxy,");
        w.line(&format!("{event_type}::{variant} {{ {fields_str} }},"));
        w.dedent();
        w.line(");");
    } else {
        w.line(&format!(
            "state.event(&proxy, {event_type}::{variant} {{ {fields_str} }});"
        ));
    }

    w.dedent();
    w.line("}");
    w.line("");
}

fn write_send_method(w: &mut W, idx: usize, m: &Method) {
    let method_name = format!("send_{}", m.name);

    if m.returns.is_some() {
        // Returns an object
        let mut params_str = String::from("&self");
        for arg in &m.args {
            let _ = write!(
                params_str,
                ", {}: {}",
                arg.name,
                send_param_type(&arg.arg_type, arg.interface.as_deref())
            );
        }
        w.line(&format!(
            "pub fn {method_name}({params_str}) -> Option<hyprwire::implementation::types::Object> {{"
        ));
        w.indent();

        // Build call args
        if m.args.is_empty() {
            w.line(&format!(
                "let seq = self.object.inner().borrow_mut().call({idx}, &[]);"
            ));
        } else {
            write_call_with_args(w, idx, &m.args, true);
        }

        w.line("let obj = self");
        w.indent();
        w.line(".object");
        w.line(".inner()");
        w.line(".borrow()");
        w.line(".client_sock()");
        w.line(".and_then(|sock| sock.object_for_seq(seq));");
        w.dedent();
        w.line("Some(hyprwire::implementation::types::Object::from_raw(obj?))");
        w.dedent();
        w.line("}");
    } else if m.args.is_empty() {
        // Destructor or no-arg method
        w.line(&format!("pub fn {method_name}(&self) {{"));
        w.indent();
        w.line(&format!(
            "self.object.inner().borrow_mut().call({idx}, &[]);"
        ));
        w.dedent();
        w.line("}");
    } else {
        // Regular method with args
        let mut params_str = String::from("&self");
        for arg in &m.args {
            let _ = write!(
                params_str,
                ", {}: {}",
                arg.name,
                send_param_type(&arg.arg_type, arg.interface.as_deref())
            );
        }
        w.line(&format!("pub fn {method_name}({params_str}) {{"));
        w.indent();

        // Check if any arg needs pre-processing (array varchar)
        let has_varchar_array = m.args.iter().any(|a| a.arg_type == ArgType::ArrayVarchar);
        if has_varchar_array {
            for arg in &m.args {
                if arg.arg_type == ArgType::ArrayVarchar {
                    w.line(&format!(
                        "let bytes: Vec<&[u8]> = {}.iter().map(|s| s.as_bytes()).collect();",
                        arg.name
                    ));
                }
            }
        }

        write_call_with_args(w, idx, &m.args, false);

        w.dedent();
        w.line("}");
    }
}

fn write_call_with_args(w: &mut W, idx: usize, args: &[super::parse::Arg], is_seq: bool) {
    let call_args: Vec<String> = args
        .iter()
        .map(|a| call_arg_expr(&a.name, &a.arg_type))
        .collect();

    let prefix = if is_seq { "let seq = " } else { "" };

    if call_args.len() == 1 {
        let arg = &call_args[0];
        // Check if it fits on one line reasonably
        let call_line = format!("{prefix}self.object.inner().borrow_mut().call({idx}, &[{arg}]);");
        if call_line.len() < 100 {
            w.line(&call_line);
        } else {
            w.line(&format!("{prefix}self.object.inner().borrow_mut().call("));
            w.indent();
            w.line(&format!("{idx},"));
            w.line(&format!("&[{arg}],"));
            w.dedent();
            w.line(");");
        }
    } else {
        w.line(&format!("{prefix}self.object.inner().borrow_mut().call("));
        w.indent();
        w.line(&format!("{idx},"));
        w.line("&[");
        w.indent();
        for arg in &call_args {
            w.line(&format!("{arg},"));
        }
        w.dedent();
        w.line("],");
        w.dedent();
        w.line(");");
    }
}

// --- Public API ---

pub fn generate(protocol: &Protocol) -> String {
    let mut w = W::new();

    generate_server(&mut w, protocol);
    w.line("");
    generate_client(&mut w, protocol);
    w.line("");
    generate_spec(&mut w, protocol);
    w.line("");

    w.buf
}
