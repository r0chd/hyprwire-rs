use super::parse::{Arg, ArgType, Description, Method, Protocol};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

const SCANNER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Targets(u8);

impl Targets {
    pub const CLIENT: Self = Self(0b01);
    pub const SERVER: Self = Self(0b10);
    pub const ALL: Self = Self(Self::CLIENT.0 | Self::SERVER.0);

    #[must_use]
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Targets {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Targets {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

fn raw_object_type() -> TokenStream {
    quote! { rc::Rc<dyn hyprwire::implementation::object::Object> }
}

#[derive(Clone)]
struct TypeAttribute {
    path: String,
    tokens: TokenStream,
}

fn parse_type_attributes(attributes: &[(String, String)]) -> Vec<TypeAttribute> {
    attributes
        .iter()
        .map(|(path, attribute)| {
            let path = path.trim().to_string();
            let tokens = attribute
                .parse::<TokenStream>()
                .unwrap_or_else(|err| panic!("failed to parse type attribute for '{path}': {err}"));
            TypeAttribute { path, tokens }
        })
        .collect()
}

fn type_path_matches(attribute_path: &str, full_path: &str) -> bool {
    if attribute_path == "." {
        return true;
    }

    if attribute_path.starts_with('.') {
        return attribute_path == full_path;
    }

    full_path.ends_with(attribute_path)
}

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

fn magic_for_arg(arg_type: &ArgType) -> Vec<TokenStream> {
    match arg_type {
        ArgType::Varchar => {
            vec![quote! { hyprwire::core::types::MessageMagic::TypeVarchar as u8 }]
        }
        ArgType::Fd => vec![quote! { hyprwire::core::types::MessageMagic::TypeFd as u8 }],
        ArgType::Uint | ArgType::Enum => {
            vec![quote! { hyprwire::core::types::MessageMagic::TypeUint as u8 }]
        }
        ArgType::Int => {
            vec![quote! { hyprwire::core::types::MessageMagic::TypeInt as u8 }]
        }
        ArgType::F32 => {
            vec![quote! { hyprwire::core::types::MessageMagic::TypeF32 as u8 }]
        }
        ArgType::ArrayVarchar => vec![
            quote! { hyprwire::core::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::core::types::MessageMagic::TypeVarchar as u8 },
        ],
        ArgType::ArrayFd => vec![
            quote! { hyprwire::core::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::core::types::MessageMagic::TypeFd as u8 },
        ],
        ArgType::ArrayUint => vec![
            quote! { hyprwire::core::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::core::types::MessageMagic::TypeUint as u8 },
        ],
        ArgType::ArrayInt => vec![
            quote! { hyprwire::core::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::core::types::MessageMagic::TypeInt as u8 },
        ],
        ArgType::ArrayF32 => vec![
            quote! { hyprwire::core::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::core::types::MessageMagic::TypeF32 as u8 },
        ],
    }
}

fn event_field_type(arg_type: &ArgType, interface: Option<&str>) -> TokenStream {
    match arg_type {
        ArgType::Varchar => quote! { String },
        ArgType::Fd => quote! { OwnedFd },
        ArgType::Int => quote! { i32 },
        ArgType::Uint => quote! { u32 },
        ArgType::Enum => {
            let ident = format_ident!("{}", snake_to_pascal(interface.unwrap()));
            quote! { super::super::spec::#ident }
        }
        ArgType::F32 => quote! { f32 },
        ArgType::ArrayVarchar => quote! { Vec<String> },
        ArgType::ArrayFd => quote! { Vec<OwnedFd> },
        ArgType::ArrayInt => quote! { Vec<i32> },
        ArgType::ArrayUint => quote! { Vec<u32> },
        ArgType::ArrayF32 => quote! { Vec<f32> },
    }
}

fn write_parse_arg(name: &proc_macro2::Ident, arg: &Arg) -> TokenStream {
    match &arg.arg_type {
        ArgType::Varchar => {
            quote! {
                __needle += 1;
                let (__len, __vl) = hyprwire::core::message::parse_var_int(__data, __needle);
                __needle += __vl;
                let #name = String::from_utf8_lossy(&__data[__needle..__needle + __len]).into_owned();
                __needle += __len;
            }
        }
        ArgType::Uint => {
            quote! {
                __needle += 1;
                let #name = u32::from_le_bytes(__data[__needle..__needle + 4].try_into().unwrap());
                __needle += 4;
            }
        }
        ArgType::Int => {
            quote! {
                __needle += 1;
                let #name = i32::from_le_bytes(__data[__needle..__needle + 4].try_into().unwrap());
                __needle += 4;
            }
        }
        ArgType::F32 => {
            quote! {
                __needle += 1;
                let #name = f32::from_le_bytes(__data[__needle..__needle + 4].try_into().unwrap());
                __needle += 4;
            }
        }
        ArgType::Enum => {
            let ident = format_ident!("{}", snake_to_pascal(arg.interface.as_deref().unwrap()));
            quote! {
                __needle += 1;
                let #name: super::super::spec::#ident = unsafe {
                    std::mem::transmute::<u32, super::super::spec::#ident>(
                        u32::from_le_bytes(__data[__needle..__needle + 4].try_into().unwrap())
                    )
                };
                __needle += 4;
            }
        }
        ArgType::Fd => {
            quote! {
                __needle += 1;
                let #name = unsafe { OwnedFd::from_raw_fd(__fds[__fd_cursor]) };
                __fd_cursor += 1;
            }
        }
        ArgType::ArrayVarchar => {
            quote! {
                __needle += 2;
                let (__count, __cl) = hyprwire::core::message::parse_var_int(__data, __needle);
                __needle += __cl;
                let mut #name = Vec::with_capacity(__count);
                for _ in 0..__count {
                    let (__slen, __svl) = hyprwire::core::message::parse_var_int(__data, __needle);
                    __needle += __svl;
                    #name.push(String::from_utf8_lossy(&__data[__needle..__needle + __slen]).into_owned());
                    __needle += __slen;
                }
            }
        }
        ArgType::ArrayFd => {
            quote! {
                __needle += 2;
                let (__count, __cl) = hyprwire::core::message::parse_var_int(__data, __needle);
                __needle += __cl;
                let mut #name = Vec::with_capacity(__count);
                for _ in 0..__count {
                    #name.push(unsafe { OwnedFd::from_raw_fd(__fds[__fd_cursor]) });
                    __fd_cursor += 1;
                }
            }
        }
        ArgType::ArrayUint => {
            quote! {
                __needle += 2;
                let (__count, __cl) = hyprwire::core::message::parse_var_int(__data, __needle);
                __needle += __cl;
                let mut #name = Vec::with_capacity(__count);
                for _ in 0..__count {
                    #name.push(u32::from_le_bytes(__data[__needle..__needle + 4].try_into().unwrap()));
                    __needle += 4;
                }
            }
        }
        ArgType::ArrayInt => {
            quote! {
                __needle += 2;
                let (__count, __cl) = hyprwire::core::message::parse_var_int(__data, __needle);
                __needle += __cl;
                let mut #name = Vec::with_capacity(__count);
                for _ in 0..__count {
                    #name.push(i32::from_le_bytes(__data[__needle..__needle + 4].try_into().unwrap()));
                    __needle += 4;
                }
            }
        }
        ArgType::ArrayF32 => {
            quote! {
                __needle += 2;
                let (__count, __cl) = hyprwire::core::message::parse_var_int(__data, __needle);
                __needle += __cl;
                let mut #name = Vec::with_capacity(__count);
                for _ in 0..__count {
                    #name.push(f32::from_le_bytes(__data[__needle..__needle + 4].try_into().unwrap()));
                    __needle += 4;
                }
            }
        }
    }
}

fn write_object_data_impl(
    obj_name: &str,
    obj_path: &TokenStream,
    event_path: &TokenStream,
    methods: &[Method],
) -> TokenStream {
    let data_ident = format_ident!("{}ObjectData", snake_to_pascal(obj_name));

    let match_arms: Vec<TokenStream> = methods
        .iter()
        .enumerate()
        .map(|(idx, m)| {
            let idx_lit = proc_macro2::Literal::u32_suffixed(idx as u32);
            let variant_ident = format_ident!("{}", snake_to_pascal(&m.name));

            let locals_init = quote! {
                let mut __needle: usize = 0;
                let mut __fd_cursor: usize = 0;
            };

            let seq_parse = if m.returns.is_some() {
                quote! {
                    __needle += 1;
                    let seq = u32::from_le_bytes(__data[__needle..__needle + 4].try_into().unwrap());
                    __needle += 4;
                }
            } else {
                quote! {}
            };

            let parse_stmts: Vec<TokenStream> = m
                .args
                .iter()
                .map(|a| {
                    let aname = raw_ident(&a.name);
                    write_parse_arg(&aname, a)
                })
                .collect();

            let mut event_fields: Vec<TokenStream> = Vec::new();
            if m.returns.is_some() {
                event_fields.push(quote! { seq, });
            }
            for a in &m.args {
                let aname = raw_ident(&a.name);
                event_fields.push(quote! { #aname, });
            }

            let event_construct = if event_fields.is_empty() {
                quote! { #event_path::#variant_ident }
            } else {
                quote! { #event_path::#variant_ident { #(#event_fields)* } }
            };

            quote! {
                #idx_lit => {
                    #locals_init
                    #seq_parse
                    #(#parse_stmts)*
                    __dispatch.event(&__proxy, #event_construct);
                }
            }
        })
        .collect();

    quote! {
        struct #data_ident<D> {
            object: *const dyn hyprwire::implementation::object::Object,
            _phantom: std::marker::PhantomData<D>,
        }

        unsafe impl<D> Send for #data_ident<D> {}
        unsafe impl<D> Sync for #data_ident<D> {}

        impl<D: hyprwire::Dispatch<#obj_path> + 'static> hyprwire::implementation::object::ObjectData for #data_ident<D> {
            fn dispatch(&self, __method: u32, __data: &[u8], __fds: &[i32], __state: &mut dyn std::any::Any) {
                let Some(__dispatch) = __state.downcast_mut::<D>() else {
                    return;
                };
                unsafe { rc::Rc::increment_strong_count(self.object) };
                let __proxy = #obj_path {
                    object: unsafe { rc::Rc::from_raw(self.object) },
                };

                match __method {
                    #(#match_arms)*
                    _ => {}
                }
            }
        }
    }
}

fn send_param_type(arg_type: &ArgType, interface: Option<&str>) -> TokenStream {
    match arg_type {
        ArgType::Varchar => quote! { impl AsRef<str> },
        ArgType::Fd => quote! { impl AsFd },
        ArgType::Int => quote! { i32 },
        ArgType::Uint => quote! { u32 },
        ArgType::F32 => quote! { f32 },
        ArgType::Enum => {
            let ident = format_ident!("{}", snake_to_pascal(interface.unwrap()));
            quote! { super::super::spec::#ident }
        }
        ArgType::ArrayVarchar => quote! { &[S] },
        ArgType::ArrayFd => quote! { &[F] },
        ArgType::ArrayInt => quote! { &[i32] },
        ArgType::ArrayUint => quote! { &[u32] },
        ArgType::ArrayF32 => quote! { &[f32] },
    }
}

fn call_arg_expr(name_ident: &proc_macro2::Ident, arg_type: &ArgType) -> TokenStream {
    match arg_type {
        ArgType::Varchar => {
            quote! { hyprwire::core::types::CallArg::Varchar(#name_ident.as_ref().as_bytes()) }
        }
        ArgType::Fd => {
            quote! { hyprwire::core::types::CallArg::Fd(#name_ident.as_fd().as_raw_fd()) }
        }
        ArgType::Uint => quote! { hyprwire::core::types::CallArg::Uint(#name_ident) },
        ArgType::Int => quote! { hyprwire::core::types::CallArg::Int(#name_ident) },
        ArgType::F32 => quote! { hyprwire::core::types::CallArg::F32(#name_ident) },
        ArgType::Enum => {
            quote! { hyprwire::core::types::CallArg::Uint(#name_ident as u32) }
        }
        ArgType::ArrayVarchar => {
            quote! {
                hyprwire::core::types::CallArg::VarcharArray(
                    &#name_ident
                        .iter()
                        .map(|s| s.as_ref().as_bytes())
                        .collect::<Vec<_>>(),
                )
            }
        }
        ArgType::ArrayFd => {
            let raw_name = format_ident!("{}_raw_fds", name_ident);
            quote! { hyprwire::core::types::CallArg::FdArray(&#raw_name) }
        }
        ArgType::ArrayUint => {
            quote! { hyprwire::core::types::CallArg::UintArray(#name_ident) }
        }
        ArgType::ArrayInt => {
            quote! { hyprwire::core::types::CallArg::IntArray(#name_ident) }
        }
        ArgType::ArrayF32 => {
            quote! { hyprwire::core::types::CallArg::F32Array(#name_ident) }
        }
    }
}

fn raw_ident(name: &str) -> proc_macro2::Ident {
    // r# prefix for reserved keywords (strict + reserved-for-future-use)
    let reserved = matches!(
        name,
        "as" | "async"
            | "await"
            | "break"
            | "become"
            | "box"
            | "const"
            | "continue"
            | "crate"
            | "do"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
    );
    if reserved {
        format_ident!("r#{}", name)
    } else {
        format_ident!("{}", name)
    }
}

fn trim_doc_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().map(|line| line.trim().to_string()).collect();
    while matches!(lines.first(), Some(line) if line.is_empty()) {
        lines.remove(0);
    }
    while matches!(lines.last(), Some(line) if line.is_empty()) {
        lines.pop();
    }
    lines
}

fn description_doc_lines(description: Option<&Description>) -> Vec<String> {
    let Some(description) = description else {
        return Vec::new();
    };

    let mut lines = Vec::new();
    if let Some(summary) = description.summary.as_deref() {
        let summary = summary.trim();
        if !summary.is_empty() {
            lines.push(summary.to_string());
        }
    }

    if let Some(body) = description.body.as_deref() {
        let body_lines = trim_doc_lines(body);
        if !body_lines.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.extend(body_lines);
        }
    }

    lines
}

fn doc_attrs(lines: &[String]) -> TokenStream {
    if lines.is_empty() {
        return quote! {};
    }
    let formatted: Vec<String> = lines
        .iter()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!(" {line}")
            }
        })
        .collect();
    quote! {
        #(#[doc = #formatted])*
    }
}

fn object_doc_attrs(description: Option<&Description>) -> TokenStream {
    doc_attrs(&description_doc_lines(description))
}

fn method_doc_attrs(method: &Method, returns_named_object: bool) -> TokenStream {
    let mut lines = description_doc_lines(method.description.as_ref());

    let arg_lines: Vec<String> = method.args.iter().map(arg_doc_line).collect();
    if !arg_lines.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Arguments:".to_string());
        lines.extend(arg_lines);
    }

    if returns_named_object {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        let returned = format!(
            "Returns a new `{}` object.",
            method.returns.as_deref().expect("checked above")
        );
        lines.push(returned);
    }

    doc_attrs(&lines)
}

fn arg_doc_line(arg: &Arg) -> String {
    match arg
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(summary) => format!("`{}`: {}", arg.name, summary),
        None => format!("`{}`.", arg.name),
    }
}

fn write_method_spec(idx: usize, m: &Method) -> TokenStream {
    let idx_lit = proc_macro2::Literal::u32_suffixed(idx as u32);
    let params: Vec<TokenStream> = m
        .args
        .iter()
        .flat_map(|arg| magic_for_arg(&arg.arg_type))
        .collect();

    let params_ts = if params.is_empty() {
        quote! { params: &[], }
    } else {
        quote! { params: &[#(#params),*], }
    };

    let ret = m.returns.as_deref().unwrap_or("");
    let destructor = m.destructor;
    quote! {
        hyprwire::core::types::Method {
            idx: #idx_lit,
            #params_ts
            returns_type: #ret,
            since: 0,
            destructor: #destructor,
        },
    }
}

fn generate_spec(protocol: &Protocol, type_attributes: &[TypeAttribute]) -> TokenStream {
    let enum_items: Vec<TokenStream> = protocol
        .enums
        .iter()
        .map(|e| {
            let ident = format_ident!("{}", snake_to_pascal(&e.name));
            let variants: Vec<TokenStream> = e
                .values
                .iter()
                .map(|v| {
                    let name = format_ident!("{}", snake_to_pascal(&v.name));
                    let idx = v.idx;
                    quote! { #name = #idx, }
                })
                .collect();
            let type_name = &e.name;
            let full_path = format!(".{}.{}", protocol.name, type_name);
            let enum_attributes: Vec<TokenStream> = type_attributes
                .iter()
                .filter(|attr| type_path_matches(&attr.path, &full_path))
                .map(|attr| attr.tokens.clone())
                .collect();
            quote! {
                #(#enum_attributes)*
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
                #[repr(u32)]
                pub enum #ident {
                    #(#variants)*
                }
            }
        })
        .collect();

    let object_items: Vec<TokenStream> = protocol.objects.iter().map(|obj| {
        let pascal = snake_to_pascal(&obj.name);
        let spec_ident = format_ident!("{}Spec", pascal);
        let static_ident = format_ident!("{}", snake_to_screaming(&obj.name));
        let obj_name_str = &obj.name;

        let c2s_specs: Vec<TokenStream> = obj.c2s.iter().enumerate()
            .map(|(idx, m)| write_method_spec(idx, m)).collect();
        let s2c_specs: Vec<TokenStream> = obj.s2c.iter().enumerate()
            .map(|(idx, m)| write_method_spec(idx, m)).collect();

        quote! {
            pub struct #spec_ident {
                c2s_methods: &'static [hyprwire::core::types::Method],
                s2c_methods: &'static [hyprwire::core::types::Method],
            }

            static #static_ident: std::sync::LazyLock<std::sync::Arc<dyn hyprwire::core::types::ProtocolObjectSpec>> =
                std::sync::LazyLock::new(|| std::sync::Arc::new(#spec_ident {
                    c2s_methods: &[#(#c2s_specs)*],
                    s2c_methods: &[#(#s2c_specs)*],
                }));

            impl hyprwire::core::types::ProtocolObjectSpec for #spec_ident {
                fn object_name(&self) -> &str { #obj_name_str }
                fn c2s(&self) -> &[hyprwire::core::types::Method] { self.c2s_methods }
                fn s2c(&self) -> &[hyprwire::core::types::Method] { self.s2c_methods }
            }
        }
    }).collect();

    let proto_pascal = snake_to_pascal(&protocol.name);
    let proto_spec_ident = format_ident!("{}ProtocolSpec", proto_pascal);
    let proto_name_str = &protocol.name;
    let proto_ver = protocol.version;
    let num_objects = protocol.objects.len();
    let obj_arc_clones: Vec<TokenStream> = protocol
        .objects
        .iter()
        .map(|o| {
            let static_ident = format_ident!("{}", snake_to_screaming(&o.name));
            quote! { std::sync::Arc::clone(&#static_ident) }
        })
        .collect();

    quote! {
        #[allow(clippy::all, dead_code)]
        mod spec {
            #(#enum_items)*

            #(#object_items)*

            #[derive(Clone)]
            pub struct #proto_spec_ident {
                objects: [std::sync::Arc<dyn hyprwire::core::types::ProtocolObjectSpec>; #num_objects],
            }

            impl Default for #proto_spec_ident {
                fn default() -> Self {
                    Self {
                        objects: [#(#obj_arc_clones),*],
                    }
                }
            }

            impl hyprwire::core::types::ProtocolSpec for #proto_spec_ident {
                fn spec_name(&self) -> &str { #proto_name_str }
                fn spec_ver(&self) -> u32 { #proto_ver }
                fn objects(&self) -> &[std::sync::Arc<dyn hyprwire::core::types::ProtocolObjectSpec>] {
                    &self.objects
                }
            }
        }
    }
}

fn write_event_enum(event_ident: &proc_macro2::Ident, methods: &[Method]) -> TokenStream {
    let variants: Vec<TokenStream> = methods
        .iter()
        .map(|m| {
            let variant = format_ident!("{}", snake_to_pascal(&m.name));
            let method_docs = object_doc_attrs(m.description.as_ref());
            if m.args.is_empty() && m.returns.is_some() {
                quote! { #method_docs #variant { seq: u32 }, }
            } else if m.args.is_empty() {
                quote! { #method_docs #variant, }
            } else {
                let fields: Vec<TokenStream> = m
                    .args
                    .iter()
                    .map(|a| {
                        let fname = raw_ident(&a.name);
                        let ftype = event_field_type(&a.arg_type, a.interface.as_deref());
                        quote! { #fname: #ftype, }
                    })
                    .collect();
                if m.returns.is_some() {
                    quote! { #method_docs #variant { seq: u32, #(#fields)* }, }
                } else {
                    quote! { #method_docs #variant { #(#fields)* }, }
                }
            }
        })
        .collect();

    quote! {
        #[non_exhaustive]
        #[derive(Debug)]
        pub enum #event_ident {
            #(#variants)*
        }
    }
}

fn write_send_method(idx: usize, m: &Method) -> TokenStream {
    let method_ident = format_ident!("send_{}", m.name);
    let idx_lit = proc_macro2::Literal::u32_suffixed(idx as u32);
    let docs = method_doc_attrs(m, m.returns.is_some());
    let has_varchar_array = m.args.iter().any(|a| a.arg_type == ArgType::ArrayVarchar);
    let has_fd_array = m.args.iter().any(|a| a.arg_type == ArgType::ArrayFd);
    let s_bound = if has_varchar_array {
        quote! { S: AsRef<str>, }
    } else {
        quote! {}
    };
    let f_bound = if has_fd_array {
        quote! { F: AsFd, }
    } else {
        quote! {}
    };

    let param_pairs: Vec<TokenStream> = m
        .args
        .iter()
        .map(|arg| {
            let aname = raw_ident(&arg.name);
            let atype = send_param_type(&arg.arg_type, arg.interface.as_deref());
            quote! { #aname: #atype }
        })
        .collect();

    if m.returns.is_some() {
        let call_body = build_call_body(idx, &m.args, true);
        let returned_mod_ident = raw_ident(m.returns.as_deref().expect("checked above"));
        let returned_obj_ident = format_ident!(
            "{}",
            snake_to_pascal(m.returns.as_deref().expect("checked above"))
        );
        let returned_obj_path = quote! { super::#returned_mod_ident::#returned_obj_ident };
        quote! {
            #docs
            pub fn #method_ident<#s_bound #f_bound D: hyprwire::Dispatch<#returned_obj_path> + 'static>(
                &self,
                #(#param_pairs,)*
            ) -> Option<#returned_obj_path> {
                #call_body
                let obj = self
                    .object
                    .client_sock()
                    .and_then(|sock| sock.object_for_seq(seq));
                Some(<#returned_obj_path as hyprwire::Object>::from_object::<D>(obj?))
            }
        }
    } else if m.destructor {
        let call_body = build_call_body(idx, &m.args, false);
        quote! {
            #docs
            pub fn #method_ident<#s_bound #f_bound>(mut self, #(#param_pairs,)*) {
                #call_body
            }
        }
    } else if m.args.is_empty() {
        quote! {
            #docs
            pub fn #method_ident(&self) {
                self.object.call(#idx_lit, &[]);
            }
        }
    } else {
        let call_body = build_call_body(idx, &m.args, false);
        quote! {
            #docs
            pub fn #method_ident<#s_bound #f_bound>(
                &self,
                #(#param_pairs,)*
            ) {
                #call_body
            }
        }
    }
}

fn write_server_create_helper(m: &Method) -> Option<TokenStream> {
    let returned = m.returns.as_deref()?;
    let helper_ident = raw_ident(&m.name);
    let returned_mod_ident = raw_ident(returned);
    let returned_obj_ident = format_ident!("{}", snake_to_pascal(returned));
    let returned_obj_path = quote! { super::#returned_mod_ident::#returned_obj_ident };
    let docs = method_doc_attrs(m, true);
    Some(quote! {
        #docs
        pub fn #helper_ident<D: hyprwire::Dispatch<#returned_obj_path> + 'static>(
            &self,
            seq: u32,
        ) -> Option<#returned_obj_path> {
            let obj = self.object.create_object(#returned, seq)?;
            Some(<#returned_obj_path as hyprwire::Object>::from_object::<D>(obj))
        }
    })
}

fn build_call_body(idx: usize, args: &[super::parse::Arg], is_seq: bool) -> TokenStream {
    let idx_lit = proc_macro2::Literal::u32_suffixed(idx as u32);

    let prep: Vec<TokenStream> = args
        .iter()
        .filter_map(|arg| {
            let aname = raw_ident(&arg.name);
            match &arg.arg_type {
                ArgType::ArrayFd => {
                    let raw_name = format_ident!("{}_raw_fds", aname);
                    Some(quote! {
                        let #raw_name: Vec<i32> = #aname.iter().map(|f| f.as_fd().as_raw_fd()).collect();
                    })
                }
                _ => None,
            }
        })
        .collect();

    let call_args: Vec<TokenStream> = args
        .iter()
        .map(|a| {
            let aname = raw_ident(&a.name);
            call_arg_expr(&aname, &a.arg_type)
        })
        .collect();

    if is_seq {
        quote! {
            #(#prep)*
            let seq = self.object.call(#idx_lit, &[#(#call_args),*]);
        }
    } else {
        quote! {
            #(#prep)*
            self.object.call(#idx_lit, &[#(#call_args),*]);
        }
    }
}

fn write_new_fn(obj_name: &str) -> TokenStream {
    let data_ident = format_ident!("{}ObjectData", snake_to_pascal(obj_name));
    let raw_obj = raw_object_type();

    quote! {
        pub fn new<D: hyprwire::Dispatch<Self> + 'static>(
            object: #raw_obj,
        ) -> Self {
            let object_data: Box<dyn hyprwire::implementation::object::ObjectData> = Box::new(#data_ident::<D> {
                object: rc::Rc::as_ptr(&object),
                _phantom: std::marker::PhantomData,
            });
            object.set_object_data(object_data);

            Self { object }
        }
    }
}

fn generate_server(protocol: &Protocol) -> TokenStream {
    let mut items: Vec<TokenStream> = Vec::new();

    for obj in &protocol.objects {
        let obj_mod_ident = raw_ident(&obj.name);
        let obj_type_ident = format_ident!("{}", snake_to_pascal(&obj.name));
        let raw_obj = raw_object_type();
        let docs = object_doc_attrs(obj.description.as_ref());
        let obj_name_str = &obj.name;
        let event_docs = format!(
            "Incoming events for `{obj_mod_ident}::{}`.",
            snake_to_pascal(&obj.name)
        );
        let event_ident = format_ident!("Event");
        let event_enum = write_event_enum(&event_ident, &obj.c2s);
        let obj_path = quote! { #obj_mod_ident::#obj_type_ident };
        let event_path = quote! { #obj_mod_ident::Event };
        let object_data_impl = write_object_data_impl(&obj.name, &obj_path, &event_path, &obj.c2s);
        let new_fn = write_new_fn(&obj.name);
        let create_helpers: Vec<TokenStream> = obj
            .c2s
            .iter()
            .filter_map(write_server_create_helper)
            .collect();
        let send_methods: Vec<TokenStream> = obj
            .s2c
            .iter()
            .enumerate()
            .map(|(idx, m)| write_send_method(idx, m))
            .collect();

        items.push(quote! {
            pub mod #obj_mod_ident {
                use super::*;

                #docs
                pub struct #obj_type_ident {
                    pub(super) object: #raw_obj,
                }

                impl std::fmt::Debug for #obj_type_ident {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.debug_struct("Object").field("object", &rc::Rc::as_ptr(&self.object)).finish()
                    }
                }

                impl Clone for #obj_type_ident {
                    fn clone(&self) -> Self {
                        Self { object: rc::Rc::clone(&self.object) }
                    }
                }

                impl PartialEq for #obj_type_ident {
                    fn eq(&self, other: &Self) -> bool { rc::Rc::ptr_eq(&self.object, &other.object) }
                }

                impl Eq for #obj_type_ident {}

                impl std::hash::Hash for #obj_type_ident {
                    fn hash<H: std::hash::Hasher>(&self, state: &mut H) { rc::Rc::as_ptr(&self.object).hash(state); }
                }

                #[doc = #event_docs]
                #event_enum

                #object_data_impl

                impl hyprwire::Object for #obj_type_ident {
                    type Event<'a> = Event;
                    const NAME: &str = #obj_name_str;
                    fn from_object<D: hyprwire::Dispatch<Self> + 'static>(object: #raw_obj) -> Self {
                        Self::new::<D>(object)
                    }
                }

                impl #obj_type_ident {
                    #new_fn

                    pub fn error(&self, error_id: u32, error_msg: impl AsRef<str>) {
                        self.object.error(error_id, error_msg.as_ref());
                    }

                    pub fn client(&self) -> Option<hyprwire::server::ServerClient> {
                        self.object.server_client()
                    }

                    #(#create_helpers)*

                    #(#send_methods)*
                }
            }
        });
    }

    let proto_pascal = snake_to_pascal(&protocol.name);
    let handler_ident = format_ident!("{}Handler", proto_pascal);
    let impl_ident = format_ident!("{}Impl", proto_pascal);
    let proto_spec_ident = format_ident!("{}ProtocolSpec", proto_pascal);

    let first_obj_mod_ident = raw_ident(&protocol.objects[0].name);
    let first_obj_type_ident = format_ident!("{}", snake_to_pascal(&protocol.objects[0].name));
    let first_obj_path = quote! { #first_obj_mod_ident::#first_obj_type_ident };

    let obj_impls: Vec<TokenStream> = protocol
        .objects
        .iter()
        .enumerate()
        .map(|(idx, obj)| {
            let obj_name_str = &obj.name;
            let on_bind = if idx == 0 {
                let bind_obj_mod_ident = raw_ident(&obj.name);
                let bind_obj_type_ident = format_ident!("{}", snake_to_pascal(&obj.name));
                let bind_obj_path = quote! { #bind_obj_mod_ident::#bind_obj_type_ident };
                quote! {
                    on_bind: Box::new(move |obj| {
                        let typed = #bind_obj_path::new::<D>(obj);
                        unsafe { &mut *handler }.bind(typed);
                    }),
                }
            } else {
                quote! { on_bind: Box::new(|_obj| {}), }
            };
            quote! {
                hyprwire::implementation::server::ObjectImplementation {
                    object_name: #obj_name_str,
                    version,
                    #on_bind
                },
            }
        })
        .collect();

    items.push(quote! {
        pub trait #handler_ident {
            /// Called whenever the server binds a new instance of the protocol's
            /// root object for a client.
            fn bind(&mut self, object: #first_obj_path);
        }

        pub struct #impl_ident {
            version: u32,
            handler: *mut dyn #handler_ident,
            protocol: super::spec::#proto_spec_ident,
            impls: Vec<hyprwire::implementation::server::ObjectImplementation<'static>>,
        }

        impl #impl_ident {
            pub fn new<D: #handler_ident + hyprwire::Dispatch<#first_obj_path> + 'static>(version: u32, handler: &mut D) -> Self {
                let handler = handler as *mut dyn #handler_ident;
                Self {
                    version,
                    handler,
                    protocol: super::spec::#proto_spec_ident::default(),
                    impls: vec![#(#obj_impls)*],
                }
            }
        }

        impl hyprwire::implementation::server::ProtocolImplementations for #impl_ident {
            fn protocol(&self) -> &dyn hyprwire::core::types::ProtocolSpec {
                &self.protocol
            }
            fn implementation(&self) -> &[hyprwire::implementation::server::ObjectImplementation<'_>] {
                &self.impls
            }
        }

        impl<D> hyprwire::implementation::server::Construct<D> for #impl_ident
        where
            D: #handler_ident + hyprwire::Dispatch<#first_obj_path> + 'static,
        {
            fn new(version: u32, handler: &mut D) -> Self {
                Self::new(version, handler)
            }
        }
    });

    quote! {
        #[allow(clippy::all, dead_code, unused_imports)]
        pub mod server {
            use std::{os::fd::*, rc, sync};

            #(#items)*
        }
    }
}

fn generate_client(protocol: &Protocol) -> TokenStream {
    let mut items: Vec<TokenStream> = Vec::new();

    for obj in &protocol.objects {
        let obj_mod_ident = raw_ident(&obj.name);
        let obj_type_ident = format_ident!("{}", snake_to_pascal(&obj.name));
        let raw_obj = raw_object_type();
        let docs = object_doc_attrs(obj.description.as_ref());
        let event_docs = format!(
            "Incoming events for `{obj_mod_ident}::{}`.",
            snake_to_pascal(&obj.name)
        );
        let event_ident = format_ident!("Event");
        let event_enum = write_event_enum(&event_ident, &obj.s2c);
        let obj_name_str = &obj.name;
        let obj_path = quote! { #obj_mod_ident::#obj_type_ident };
        let event_path = quote! { #obj_mod_ident::Event };
        let object_data_impl = write_object_data_impl(&obj.name, &obj_path, &event_path, &obj.s2c);
        let new_fn = write_new_fn(&obj.name);
        let send_methods: Vec<TokenStream> = obj
            .c2s
            .iter()
            .enumerate()
            .filter(|(_, m)| !(m.destructor && m.args.is_empty()))
            .map(|(idx, m)| write_send_method(idx, m))
            .collect();

        items.push(quote! {
            pub mod #obj_mod_ident {
                use super::*;

                #docs
                pub struct #obj_type_ident {
                    pub(super) object: #raw_obj,
                }

                impl std::fmt::Debug for #obj_type_ident {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.debug_struct("Object").field("object", &rc::Rc::as_ptr(&self.object)).finish()
                    }
                }

                impl Clone for #obj_type_ident {
                    fn clone(&self) -> Self {
                        Self { object: rc::Rc::clone(&self.object) }
                    }
                }

                impl PartialEq for #obj_type_ident {
                    fn eq(&self, other: &Self) -> bool { rc::Rc::ptr_eq(&self.object, &other.object) }
                }

                impl Eq for #obj_type_ident {}

                impl std::hash::Hash for #obj_type_ident {
                    fn hash<H: std::hash::Hasher>(&self, state: &mut H) { rc::Rc::as_ptr(&self.object).hash(state); }
                }

                #[doc = #event_docs]
                #event_enum

                #object_data_impl

                impl hyprwire::Object for #obj_type_ident {
                    type Event<'a> = Event;
                    const NAME: &str = #obj_name_str;
                    fn from_object<D: hyprwire::Dispatch<Self> + 'static>(object: #raw_obj) -> Self {
                        Self::new::<D>(object)
                    }
                }

                impl #obj_type_ident {
                    #new_fn

                    #(#send_methods)*
                }
            }
        });
    }

    let proto_pascal = snake_to_pascal(&protocol.name);
    let proto_impl_ident = format_ident!("{}Impl", proto_pascal);
    let proto_spec_ident = format_ident!("{}ProtocolSpec", proto_pascal);
    let protocol_name = &protocol.name;

    items.push(quote! {
        #[derive(Default, Clone)]
        pub struct #proto_impl_ident {
            protocol: super::spec::#proto_spec_ident,
        }

        impl hyprwire::implementation::client::ProtocolImplementations for #proto_impl_ident {
            fn new() -> Self {
                Self::default()
            }

            fn protocol_spec() -> Box<dyn hyprwire::core::types::ProtocolSpec> {
                Box::new(super::spec::#proto_spec_ident::default())
            }

            fn spec_name() -> &'static str {
                #protocol_name
            }

            fn protocol(&self) -> &dyn hyprwire::core::types::ProtocolSpec {
                &self.protocol
            }
            fn implementation(&self) -> &[hyprwire::implementation::client::ObjectImplementation<'_>] {
                &[]
            }
        }
    });

    quote! {
        #[allow(clippy::all, dead_code, unused_imports)]
        pub mod client {
            use std::{os::fd::*, rc, sync};

            #(#items)*
        }
    }
}

#[must_use]
pub fn generate(
    protocol: &Protocol,
    targets: Targets,
    type_attributes: &[(String, String)],
) -> String {
    let header_comment = format!(
        "// Generated with hyprwire-scanner {SCANNER_VERSION}. Made with pure malice and hatred by r0chd.\n// {}\n",
        protocol.name
    );

    let copyright_block = if let Some(copyright) = protocol.copyright.as_deref() {
        let mut lines: Vec<&str> = copyright.lines().collect();
        while matches!(lines.first(), Some(l) if l.trim().is_empty()) {
            lines.remove(0);
        }
        while matches!(lines.last(), Some(l) if l.trim().is_empty()) {
            lines.pop();
        }
        let formatted: Vec<String> = lines
            .iter()
            .map(|l| {
                let t = l.trim();
                if t.is_empty() {
                    String::new()
                } else {
                    format!("    {t}")
                }
            })
            .collect();
        format!(
            "/*\n This protocol's author copyright notice is:\n\n{}\n\n*/\n\n",
            formatted.join("\n")
        )
    } else {
        String::new()
    };

    let parsed_type_attributes = parse_type_attributes(type_attributes);
    let spec = generate_spec(protocol, &parsed_type_attributes);
    let server = targets
        .contains(Targets::SERVER)
        .then(|| generate_server(protocol));
    let client = targets
        .contains(Targets::CLIENT)
        .then(|| generate_client(protocol));

    let ts = quote! { #server #client #spec };
    let file = syn::parse_file(&ts.to_string()).expect("generated code is not valid Rust");
    let formatted = prettyplease::unparse(&file);

    format!("{header_comment}{copyright_block}{formatted}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Enum, EnumValue, Object, Protocol};

    fn sample_object() -> Object {
        Object {
            name: "dummy_object".to_string(),
            version: 1,
            c2s: Vec::new(),
            s2c: Vec::new(),
            description: None,
        }
    }

    fn sample_protocol() -> Protocol {
        Protocol {
            name: "simple".to_string(),
            version: 1,
            objects: vec![sample_object()],
            enums: vec![Enum {
                name: "my_enum".to_string(),
                values: vec![EnumValue {
                    name: "first".to_string(),
                    idx: 0,
                    description: None,
                }],
            }],
            copyright: None,
        }
    }

    #[test]
    fn type_attribute_suffix_match() {
        let protocol = sample_protocol();
        let attributes = vec![("my_enum".to_string(), "#[allow(dead_code)]".to_string())];
        let code = generate(&protocol, Targets::ALL, &attributes);
        assert!(code.contains("#[allow(dead_code)]"));
    }

    #[test]
    fn type_attribute_exact_match_with_dot() {
        let protocol = sample_protocol();
        let attributes = vec![(".simple.my_enum".to_string(), "#[doc(hidden)]".to_string())];
        let code = generate(&protocol, Targets::ALL, &attributes);
        assert!(code.contains("#[doc(hidden)]"));
    }

    #[test]
    fn type_attribute_dot_path_matches_everything() {
        let protocol = sample_protocol();
        let attributes = vec![(".".to_string(), "#[cfg(test)]".to_string())];
        let code = generate(&protocol, Targets::ALL, &attributes);
        assert!(code.contains("#[cfg(test)]"));
    }

    #[test]
    fn type_attribute_dot_path_requires_exact_match() {
        let protocol = sample_protocol();
        let attributes = vec![(".other.my_enum".to_string(), "#[test_attr]".to_string())];
        let code = generate(&protocol, Targets::ALL, &attributes);
        assert!(!code.contains("#[test_attr]"));
    }
}
