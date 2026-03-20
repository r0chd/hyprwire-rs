use super::parse::{ArgType, Method, Protocol};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

const SCANNER_VERSION: &str = env!("CARGO_PKG_VERSION");

fn raw_object_type() -> TokenStream {
    quote! { rc::Rc<cell::RefCell<dyn hyprwire::implementation::object::RawObject>> }
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

fn magic_for_arg(arg_type: &ArgType) -> Vec<TokenStream> {
    match arg_type {
        ArgType::Varchar => {
            vec![quote! { hyprwire::implementation::types::MessageMagic::TypeVarchar as u8 }]
        }
        ArgType::Fd => vec![quote! { hyprwire::implementation::types::MessageMagic::TypeFd as u8 }],
        ArgType::Uint | ArgType::Enum => {
            vec![quote! { hyprwire::implementation::types::MessageMagic::TypeUint as u8 }]
        }
        ArgType::Int => {
            vec![quote! { hyprwire::implementation::types::MessageMagic::TypeInt as u8 }]
        }
        ArgType::F32 => {
            vec![quote! { hyprwire::implementation::types::MessageMagic::TypeF32 as u8 }]
        }
        ArgType::ArrayVarchar => vec![
            quote! { hyprwire::implementation::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::implementation::types::MessageMagic::TypeVarchar as u8 },
        ],
        ArgType::ArrayFd => vec![
            quote! { hyprwire::implementation::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::implementation::types::MessageMagic::TypeFd as u8 },
        ],
        ArgType::ArrayUint => vec![
            quote! { hyprwire::implementation::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::implementation::types::MessageMagic::TypeUint as u8 },
        ],
        ArgType::ArrayInt => vec![
            quote! { hyprwire::implementation::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::implementation::types::MessageMagic::TypeInt as u8 },
        ],
        ArgType::ArrayF32 => vec![
            quote! { hyprwire::implementation::types::MessageMagic::TypeArray as u8 },
            quote! { hyprwire::implementation::types::MessageMagic::TypeF32 as u8 },
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
            quote! { super::spec::#ident }
        }
        ArgType::F32 => quote! { f32 },
        ArgType::ArrayVarchar => quote! { Vec<String> },
        ArgType::ArrayFd => quote! { Vec<OwnedFd> },
        ArgType::ArrayInt => quote! { Vec<i32> },
        ArgType::ArrayUint => quote! { Vec<u32> },
        ArgType::ArrayF32 => quote! { Vec<f32> },
    }
}

fn dispatch_param_type(arg_type: &ArgType, interface: Option<&str>) -> TokenStream {
    match arg_type {
        ArgType::Varchar => quote! { *const ffi::c_char },
        ArgType::Fd | ArgType::Int => quote! { i32 },
        ArgType::Uint => quote! { u32 },
        ArgType::Enum => {
            let ident = format_ident!("{}", snake_to_pascal(interface.unwrap()));
            quote! { super::spec::#ident }
        }
        ArgType::F32 => quote! { f32 },
        ArgType::ArrayVarchar => quote! { *const *const ffi::c_char },
        ArgType::ArrayFd | ArgType::ArrayInt => quote! { *const i32 },
        ArgType::ArrayUint => quote! { *const u32 },
        ArgType::ArrayF32 => quote! { *const f32 },
    }
}

fn send_param_type(arg_type: &ArgType, interface: Option<&str>) -> TokenStream {
    match arg_type {
        ArgType::Varchar => quote! { &str },
        ArgType::Fd => quote! { impl AsFd },
        ArgType::Int => quote! { i32 },
        ArgType::Uint => quote! { u32 },
        ArgType::F32 => quote! { f32 },
        ArgType::Enum => {
            let ident = format_ident!("{}", snake_to_pascal(interface.unwrap()));
            quote! { super::spec::#ident }
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
            quote! { hyprwire::implementation::types::CallArg::Varchar(#name_ident.as_bytes()) }
        }
        ArgType::Fd => {
            quote! { hyprwire::implementation::types::CallArg::Fd(#name_ident.as_fd().as_raw_fd()) }
        }
        ArgType::Uint => quote! { hyprwire::implementation::types::CallArg::Uint(#name_ident) },
        ArgType::Int => quote! { hyprwire::implementation::types::CallArg::Int(#name_ident) },
        ArgType::F32 => quote! { hyprwire::implementation::types::CallArg::F32(#name_ident) },
        ArgType::Enum => {
            quote! { hyprwire::implementation::types::CallArg::Uint(#name_ident as u32) }
        }
        ArgType::ArrayVarchar => {
            quote! { hyprwire::implementation::types::CallArg::VarcharArray(&bytes) }
        }
        ArgType::ArrayFd => {
            let raw_name = format_ident!("{}_raw_fds", name_ident);
            quote! { hyprwire::implementation::types::CallArg::FdArray(&#raw_name) }
        }
        ArgType::ArrayUint => {
            quote! { hyprwire::implementation::types::CallArg::UintArray(#name_ident) }
        }
        ArgType::ArrayInt => {
            quote! { hyprwire::implementation::types::CallArg::IntArray(#name_ident) }
        }
        ArgType::ArrayF32 => {
            quote! { hyprwire::implementation::types::CallArg::F32Array(#name_ident) }
        }
    }
}

fn raw_ident(name: &str) -> proc_macro2::Ident {
    // r# prefix for reserved keywords
    let reserved = matches!(
        name,
        "type"
            | "ref"
            | "move"
            | "fn"
            | "let"
            | "use"
            | "mod"
            | "pub"
            | "impl"
            | "trait"
            | "struct"
            | "enum"
            | "match"
            | "if"
            | "else"
            | "for"
            | "while"
            | "loop"
            | "return"
            | "break"
            | "continue"
            | "where"
            | "async"
            | "await"
            | "dyn"
            | "box"
            | "self"
            | "super"
            | "crate"
            | "in"
            | "as"
            | "const"
            | "static"
            | "unsafe"
            | "extern"
            | "true"
            | "false"
    );
    if reserved {
        format_ident!("r#{}", name)
    } else {
        format_ident!("{}", name)
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
        hyprwire::implementation::types::Method {
            idx: #idx_lit,
            #params_ts
            returns_type: #ret,
            since: 0,
            destructor: #destructor,
        },
    }
}

fn generate_spec(protocol: &Protocol) -> TokenStream {
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
            quote! {
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
                c2s_methods: &'static [hyprwire::implementation::types::Method],
                s2c_methods: &'static [hyprwire::implementation::types::Method],
            }

            static #static_ident: std::sync::LazyLock<std::sync::Arc<dyn hyprwire::implementation::types::ProtocolObjectSpec>> =
                std::sync::LazyLock::new(|| std::sync::Arc::new(#spec_ident {
                    c2s_methods: &[#(#c2s_specs)*],
                    s2c_methods: &[#(#s2c_specs)*],
                }));

            impl hyprwire::implementation::types::ProtocolObjectSpec for #spec_ident {
                fn object_name(&self) -> &str { #obj_name_str }
                fn c2s(&self) -> &[hyprwire::implementation::types::Method] { self.c2s_methods }
                fn s2c(&self) -> &[hyprwire::implementation::types::Method] { self.s2c_methods }
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
        #[allow(dead_code)]
        pub mod spec {
            #(#enum_items)*

            #(#object_items)*

            #[derive(Clone)]
            pub struct #proto_spec_ident {
                objects: [std::sync::Arc<dyn hyprwire::implementation::types::ProtocolObjectSpec>; #num_objects],
            }

            impl Default for #proto_spec_ident {
                fn default() -> Self {
                    Self {
                        objects: [#(#obj_arc_clones),*],
                    }
                }
            }

            impl hyprwire::implementation::types::ProtocolSpec for #proto_spec_ident {
                fn spec_name(&self) -> &str { #proto_name_str }
                fn spec_ver(&self) -> u32 { #proto_ver }
                fn objects(&self) -> &[std::sync::Arc<dyn hyprwire::implementation::types::ProtocolObjectSpec>] {
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
            if m.args.is_empty() && m.returns.is_some() {
                quote! { #variant { seq: u32 }, }
            } else if m.args.is_empty() {
                quote! { #variant, }
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
                    quote! { #variant { seq: u32, #(#fields)* }, }
                } else {
                    quote! { #variant { #(#fields)* }, }
                }
            }
        })
        .collect();

    quote! {
        #[derive(Debug)]
        pub enum #event_ident {
            #(#variants)*
        }
    }
}

fn write_dispatch_fn(
    obj_name: &str,
    obj_ident: &proc_macro2::Ident,
    event_ident: &proc_macro2::Ident,
    idx: usize,
    m: &Method,
    has_on_destroy: bool,
) -> TokenStream {
    let fn_ident = format_ident!("{}_method{}", obj_name, idx);
    let variant_ident = format_ident!("{}", snake_to_pascal(&m.name));

    let mut fn_params: Vec<TokenStream> = vec![quote! { data: *mut ffi::c_void }];
    if m.returns.is_some() {
        fn_params.push(quote! { seq: u32 });
    }
    for arg in &m.args {
        let aname = raw_ident(&arg.name);
        let atype = dispatch_param_type(&arg.arg_type, arg.interface.as_deref());
        fn_params.push(quote! { #aname: #atype });
        if is_array_type(&arg.arg_type) {
            let len_ident = format_ident!("{}_len", aname);
            fn_params.push(quote! { #len_ident: u32 });
        }
    }

    let on_destroy_field = if has_on_destroy {
        quote! { on_destroy: None, owned: false, }
    } else {
        quote! {}
    };

    let mut conversions: Vec<TokenStream> = Vec::new();
    let mut event_fields: Vec<TokenStream> = Vec::new();

    if m.returns.is_some() {
        event_fields.push(quote! { seq, });
    }

    for arg in &m.args {
        let aname = raw_ident(&arg.name);
        match &arg.arg_type {
            ArgType::Varchar => {
                conversions.push(quote! {
                    let #aname = unsafe { ffi::CStr::from_ptr(#aname) }.to_string_lossy().into_owned();
                });
                event_fields.push(quote! { #aname, });
            }
            ArgType::Fd => {
                conversions.push(quote! {
                    let #aname = unsafe { OwnedFd::from_raw_fd(#aname) };
                });
                event_fields.push(quote! { #aname, });
            }
            ArgType::ArrayVarchar => {
                let len_ident = format_ident!("{}_len", aname);
                conversions.push(quote! {
                    let #aname: Vec<String> = unsafe { std::slice::from_raw_parts(#aname, #len_ident as usize) }
                        .iter()
                        .map(|&p| unsafe { ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned())
                        .collect();
                });
                event_fields.push(quote! { #aname, });
            }
            ArgType::ArrayFd => {
                let len_ident = format_ident!("{}_len", aname);
                conversions.push(quote! {
                    let #aname: Vec<OwnedFd> = unsafe { std::slice::from_raw_parts(#aname, #len_ident as usize) }
                        .iter()
                        .map(|&fd| unsafe { OwnedFd::from_raw_fd(fd) })
                        .collect();
                });
                event_fields.push(quote! { #aname, });
            }
            t if is_array_type(t) => {
                let len_ident = format_ident!("{}_len", aname);
                conversions.push(quote! {
                    let #aname = unsafe { std::slice::from_raw_parts(#aname, #len_ident as usize) }.to_vec();
                });
                event_fields.push(quote! { #aname, });
            }
            _ => {
                event_fields.push(quote! { #aname, });
            }
        }
    }

    let dispatch_call = if event_fields.is_empty() {
        quote! { __dispatch.event(&proxy, #event_ident::#variant_ident); }
    } else {
        quote! { __dispatch.event(&proxy, #event_ident::#variant_ident { #(#event_fields)* }); }
    };

    quote! {
        unsafe extern "C" fn #fn_ident<D: hyprwire::Dispatch<#obj_ident>>(
            #(#fn_params,)*
        ) {
            let dispatch = unsafe { &*(data as *const hyprwire::DispatchData) };
            let __dispatch = unsafe { &mut *(hyprwire::get_dispatch_state() as *mut D) };
            unsafe { rc::Rc::increment_strong_count(dispatch.object) };
            let proxy = #obj_ident {
                object: unsafe { rc::Rc::from_raw(dispatch.object) },
                #on_destroy_field
            };
            #(#conversions)*
            #dispatch_call
        }
    }
}

fn write_send_method(idx: usize, m: &Method, has_on_destroy: bool) -> TokenStream {
    let method_ident = format_ident!("send_{}", m.name);
    let idx_lit = proc_macro2::Literal::u32_suffixed(idx as u32);
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
        quote! {
            pub fn #method_ident<#s_bound #f_bound T: hyprwire::Object, D: hyprwire::Dispatch<T>>(
                &self,
                #(#param_pairs,)*
            ) -> Option<T> {
                #call_body
                let obj = self
                    .object
                    .borrow()
                    .client_sock()
                    .and_then(|sock| sock.object_for_seq(seq));
                Some(T::from_object::<D>(obj?))
            }
        }
    } else if m.destructor && has_on_destroy && !m.args.is_empty() {
        let call_body = build_call_body(idx, &m.args, false);
        quote! {
            pub fn #method_ident<#s_bound #f_bound>(mut self, #(#param_pairs,)*) {
                #call_body
                if let Some(cb) = self.on_destroy.take() {
                    cb();
                }
            }
        }
    } else if m.args.is_empty() {
        quote! {
            pub fn #method_ident(&self) {
                self.object.borrow_mut().call(#idx_lit, &[]);
            }
        }
    } else {
        let call_body = build_call_body(idx, &m.args, false);
        quote! {
            pub fn #method_ident<#s_bound #f_bound>(
                &self,
                #(#param_pairs,)*
            ) {
                #call_body
            }
        }
    }
}

fn build_call_body(idx: usize, args: &[super::parse::Arg], is_seq: bool) -> TokenStream {
    let idx_lit = proc_macro2::Literal::u32_suffixed(idx as u32);

    let prep: Vec<TokenStream> = args
        .iter()
        .filter_map(|arg| {
            let aname = raw_ident(&arg.name);
            match &arg.arg_type {
                ArgType::ArrayVarchar => Some(quote! {
                    let bytes: Vec<&[u8]> = #aname.iter().map(|s| s.as_ref().as_bytes()).collect();
                }),
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
            let seq = self.object.borrow_mut().call(#idx_lit, &[#(#call_args),*]);
        }
    } else {
        quote! {
            #(#prep)*
            self.object.borrow_mut().call(#idx_lit, &[#(#call_args),*]);
        }
    }
}

fn write_new_fn(
    obj_name: &str,
    methods: &[Method],
    extra_fields: Option<TokenStream>,
) -> TokenStream {
    let listen_calls: Vec<TokenStream> = methods
        .iter()
        .enumerate()
        .map(|(idx, _m)| {
            let listen_fn = format_ident!("{}_method{}", obj_name, idx);
            let idx_lit = proc_macro2::Literal::u32_suffixed(idx as u32);
            quote! {
                obj.listen(#idx_lit, #listen_fn::<D> as *mut ffi::c_void);
            }
        })
        .collect();

    let extra = extra_fields.unwrap_or_default();

    let raw_obj = raw_object_type();
    quote! {
        pub fn new<D: hyprwire::Dispatch<Self>>(
            object: #raw_obj,
        ) -> Self {
            unsafe fn drop_dispatch_data(ptr: *mut ffi::c_void) {
                drop(unsafe { Box::from_raw(ptr as *mut hyprwire::DispatchData) });
            }

            let dispatch_data = Box::into_raw(Box::new(hyprwire::DispatchData {
                object: rc::Rc::as_ptr(&object),
            }));

            {
                let mut obj = object.borrow_mut();
                obj.set_data(dispatch_data as *mut ffi::c_void, Some(drop_dispatch_data));
                #(#listen_calls)*
            }

            Self { object, #extra }
        }
    }
}

fn generate_server(protocol: &Protocol) -> TokenStream {
    let mut items: Vec<TokenStream> = Vec::new();

    for obj in &protocol.objects {
        let obj_ident = format_ident!("{}Object", snake_to_pascal(&obj.name));
        let pascal_str = format!("{}Object", snake_to_pascal(&obj.name));
        let raw_obj = raw_object_type();
        items.push(quote! {
            pub struct #obj_ident {
                object: #raw_obj,
            }

            impl std::fmt::Debug for #obj_ident {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.debug_struct(#pascal_str).field("object", &rc::Rc::as_ptr(&self.object)).finish()
                }
            }

            impl Clone for #obj_ident {
                fn clone(&self) -> Self {
                    Self { object: rc::Rc::clone(&self.object) }
                }
            }

            impl PartialEq for #obj_ident {
                fn eq(&self, other: &Self) -> bool { rc::Rc::ptr_eq(&self.object, &other.object) }
            }

            impl Eq for #obj_ident {}

            impl std::hash::Hash for #obj_ident {
                fn hash<H: std::hash::Hasher>(&self, state: &mut H) { rc::Rc::as_ptr(&self.object).hash(state); }
            }
        });
    }

    for obj in &protocol.objects {
        let pascal = snake_to_pascal(&obj.name);
        let obj_ident = format_ident!("{}Object", pascal);
        let event_ident = format_ident!("{}Event", pascal);
        let obj_name_str = &obj.name;

        items.push(write_event_enum(&event_ident, &obj.c2s));

        {
            let raw_obj = raw_object_type();
            items.push(quote! {
                impl hyprwire::Object for #obj_ident {
                    type Event<'a> = #event_ident;
                    const NAME: &str = #obj_name_str;
                    fn from_object<D: hyprwire::Dispatch<Self>>(object: #raw_obj) -> Self {
                        Self::new::<D>(object)
                    }
                }
            });
        }

        for (idx, m) in obj.c2s.iter().enumerate() {
            items.push(write_dispatch_fn(
                &obj.name,
                &obj_ident,
                &event_ident,
                idx,
                m,
                false,
            ));
        }

        let new_fn = write_new_fn(&obj.name, &obj.c2s, None);
        let send_methods: Vec<TokenStream> = obj
            .s2c
            .iter()
            .enumerate()
            .map(|(idx, m)| write_send_method(idx, m, false))
            .collect();

        items.push(quote! {
            impl #obj_ident {
                #new_fn

                pub fn error(&self, error_id: u32, error_msg: &str) {
                    self.object.borrow().error(error_id, error_msg);
                }

                pub fn create_object<T: hyprwire::Object, D: hyprwire::Dispatch<T>>(&self, seq: u32) -> Option<T> {
                    let obj = self.object.borrow().create_object(T::NAME, seq)?;
                    Some(T::from_object::<D>(obj))
                }

                pub fn set_on_drop(&self, callback: impl FnOnce() + 'static) {
                    self.object.borrow_mut().set_on_drop(Box::new(callback));
                }

                #(#send_methods)*
            }
        });
    }

    let proto_pascal = snake_to_pascal(&protocol.name);
    let handler_ident = format_ident!("{}Handler", proto_pascal);
    let impl_ident = format_ident!("{}Impl", proto_pascal);
    let proto_spec_ident = format_ident!("{}ProtocolSpec", proto_pascal);

    let first_obj_ident = format_ident!("{}Object", snake_to_pascal(&protocol.objects[0].name));

    let obj_impls: Vec<TokenStream> = protocol
        .objects
        .iter()
        .enumerate()
        .map(|(idx, obj)| {
            let obj_name_str = &obj.name;
            let on_bind = if idx == 0 {
                let bind_obj_ident = format_ident!("{}Object", snake_to_pascal(&obj.name));
                quote! {
                    on_bind: Box::new(move |obj| {
                        let typed = #bind_obj_ident::new::<D>(obj);
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
            fn bind(&mut self, object: #first_obj_ident);
        }

        pub struct #impl_ident {
            version: u32,
            handler: *mut dyn #handler_ident,
            protocol: super::spec::#proto_spec_ident,
            impls: Vec<hyprwire::implementation::server::ObjectImplementation<'static>>,
        }

        impl #impl_ident {
            pub fn new<D: #handler_ident + hyprwire::Dispatch<#first_obj_ident> + 'static>(version: u32, handler: &mut D) -> Self {
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
            fn protocol(&self) -> &dyn hyprwire::implementation::types::ProtocolSpec {
                &self.protocol
            }
            fn implementation(&self) -> &[hyprwire::implementation::server::ObjectImplementation<'_>] {
                &self.impls
            }
        }
    });

    quote! {
        #[allow(dead_code, unused_imports)]
        pub mod server {
            use std::{cell, ffi, os::fd::*, rc};

            #(#items)*
        }
    }
}

fn generate_client(protocol: &Protocol) -> TokenStream {
    let mut items: Vec<TokenStream> = Vec::new();

    for obj in &protocol.objects {
        let pascal = snake_to_pascal(&obj.name);
        let obj_ident = format_ident!("{}Object", pascal);
        let pascal_str = format!("{}Object", pascal);
        let raw_obj = raw_object_type();

        items.push(quote! {
            pub struct #obj_ident {
                object: #raw_obj,
                on_destroy: Option<Box<dyn FnOnce()>>,
                owned: bool,
            }

            impl std::fmt::Debug for #obj_ident {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.debug_struct(#pascal_str).field("object", &rc::Rc::as_ptr(&self.object)).finish()
                }
            }

            impl Clone for #obj_ident {
                fn clone(&self) -> Self {
                    Self { object: rc::Rc::clone(&self.object), on_destroy: None, owned: false }
                }
            }

            impl PartialEq for #obj_ident {
                fn eq(&self, other: &Self) -> bool { rc::Rc::ptr_eq(&self.object, &other.object) }
            }

            impl Eq for #obj_ident {}

            impl std::hash::Hash for #obj_ident {
                fn hash<H: std::hash::Hasher>(&self, state: &mut H) { rc::Rc::as_ptr(&self.object).hash(state); }
            }
        });
    }

    for obj in &protocol.objects {
        let pascal = snake_to_pascal(&obj.name);
        let obj_ident = format_ident!("{}Object", pascal);
        let event_ident = format_ident!("{}Event", pascal);
        let obj_name_str = &obj.name;

        items.push(write_event_enum(&event_ident, &obj.s2c));

        {
            let raw_obj = raw_object_type();
            items.push(quote! {
                impl hyprwire::Object for #obj_ident {
                    type Event<'a> = #event_ident;
                    const NAME: &str = #obj_name_str;
                    fn from_object<D: hyprwire::Dispatch<Self>>(object: #raw_obj) -> Self {
                        Self::new::<D>(object)
                    }
                }
            });
        }

        for (idx, m) in obj.s2c.iter().enumerate() {
            items.push(write_dispatch_fn(
                &obj.name,
                &obj_ident,
                &event_ident,
                idx,
                m,
                true,
            ));
        }

        let new_fn = write_new_fn(
            &obj.name,
            &obj.s2c,
            Some(quote! { on_destroy: None, owned: true, }),
        );
        let send_methods: Vec<TokenStream> = obj
            .c2s
            .iter()
            .enumerate()
            .filter(|(_, m)| !(m.destructor && m.args.is_empty()))
            .map(|(idx, m)| write_send_method(idx, m, true))
            .collect();

        let auto_destructor_calls: Vec<TokenStream> = obj
            .c2s
            .iter()
            .enumerate()
            .filter(|(_, m)| m.destructor && m.args.is_empty())
            .map(|(idx, _)| {
                let idx_lit = proc_macro2::Literal::u32_suffixed(idx as u32);
                quote! { self.object.borrow_mut().call(#idx_lit, &[]); }
            })
            .collect();

        items.push(quote! {
            impl #obj_ident {
                #new_fn

                pub fn set_on_destroy(&mut self, callback: impl FnOnce() + 'static) {
                    self.on_destroy = Some(Box::new(callback));
                }

                #(#send_methods)*
            }

            impl Drop for #obj_ident {
                fn drop(&mut self) {
                    if self.owned {
                        #(#auto_destructor_calls)*
                    }
                    if let Some(cb) = self.on_destroy.take() {
                        cb();
                    }
                }
            }
        });
    }

    let proto_pascal = snake_to_pascal(&protocol.name);
    let proto_impl_ident = format_ident!("{}Impl", proto_pascal);
    let proto_spec_ident = format_ident!("{}ProtocolSpec", proto_pascal);

    items.push(quote! {
        #[derive(Default, Clone)]
        pub struct #proto_impl_ident {
            protocol: super::spec::#proto_spec_ident,
        }

        impl hyprwire::implementation::client::ProtocolImplementations for #proto_impl_ident {
            fn protocol(&self) -> &dyn hyprwire::implementation::types::ProtocolSpec {
                &self.protocol
            }
            fn implementation(&self) -> &[hyprwire::implementation::client::ObjectImplementation<'_>] {
                &[]
            }
        }
    });

    quote! {
        #[allow(dead_code, unused_imports)]
        pub mod client {
            use std::{cell, ffi, os::fd::*, rc};

            #(#items)*
        }
    }
}

#[must_use]
pub fn generate(protocol: &Protocol) -> String {
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

    let server = generate_server(protocol);
    let client = generate_client(protocol);
    let spec = generate_spec(protocol);

    let ts = quote! { #server #client #spec };
    let file = syn::parse_file(&ts.to_string()).expect("generated code is not valid Rust");
    let formatted = prettyplease::unparse(&file);

    format!("{header_comment}{copyright_block}{formatted}")
}
