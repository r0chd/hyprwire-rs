use hyprwire_scanner::generate::{self, Targets};
use hyprwire_scanner::parse::*;
use proptest::prelude::*;

fn ident_strategy() -> impl Strategy<Value = String> {
    // valid rust snake_case identifiers: lowercase ascii + underscores, must start with a letter.
    prop::string::string_regex("[a-z][a-z0-9_]{0,15}")
        .unwrap()
        .prop_filter("must not be empty", |s| !s.is_empty())
}

fn description_strategy() -> impl Strategy<Value = Option<Description>> {
    prop_oneof![
        Just(None),
        (prop::option::of(ident_strategy()), prop::option::of(".*"),)
            .prop_map(|(summary, body)| Some(Description { summary, body })),
    ]
}

fn non_enum_arg_type() -> impl Strategy<Value = ArgType> {
    prop_oneof![
        Just(ArgType::Varchar),
        Just(ArgType::Fd),
        Just(ArgType::Uint),
        Just(ArgType::Int),
        Just(ArgType::F32),
        Just(ArgType::ArrayVarchar),
        Just(ArgType::ArrayFd),
        Just(ArgType::ArrayUint),
        Just(ArgType::ArrayInt),
        Just(ArgType::ArrayF32),
    ]
}

fn arg_strategy(enum_names: Vec<String>) -> impl Strategy<Value = Arg> {
    let has_enums = !enum_names.is_empty();

    (
        ident_strategy(),
        non_enum_arg_type(),
        description_strategy(),
    )
        .prop_flat_map(move |(name, base_type, desc)| {
            let enum_names = enum_names.clone();
            let name = name.clone();
            let desc = desc.clone();

            if has_enums {
                let enum_names_inner = enum_names.clone();
                prop_oneof![
                    3 => Just((base_type.clone(), None)),
                    1 => (0..enum_names.len()).prop_map(move |idx| {
                        (ArgType::Enum, Some(enum_names_inner[idx].clone()))
                    }),
                ]
                .prop_map(move |(arg_type, interface)| Arg {
                    name: name.clone(),
                    arg_type,
                    interface,
                    summary: desc.as_ref().and_then(|d| d.summary.clone()),
                })
                .boxed()
            } else {
                Just(Arg {
                    name,
                    arg_type: base_type,
                    interface: None,
                    summary: desc.and_then(|d| d.summary),
                })
                .boxed()
            }
        })
}

fn method_strategy(
    enum_names: Vec<String>,
    object_names: Vec<String>,
) -> impl Strategy<Value = Method> {
    let has_objects = object_names.len() > 1;

    (
        ident_strategy(),
        prop::collection::vec(arg_strategy(enum_names), 0..6),
        prop::bool::ANY,
        description_strategy(),
    )
        .prop_flat_map(move |(name, args, destructor, description)| {
            let object_names = object_names.clone();
            let name = name.clone();
            let args = args.clone();
            let description = description.clone();

            if has_objects && !destructor {
                let object_names_inner = object_names.clone();
                prop_oneof![
                    3 => Just(None),
                    1 => (1..object_names.len())
                        .prop_map(move |idx| Some(object_names_inner[idx].clone())),
                ]
                .prop_map(move |returns| Method {
                    name: name.clone(),
                    args: args.clone(),
                    returns,
                    destructor,
                    description: description.clone(),
                })
                .boxed()
            } else {
                Just(Method {
                    name,
                    args,
                    returns: None,
                    destructor,
                    description,
                })
                .boxed()
            }
        })
}

fn enum_strategy() -> impl Strategy<Value = Enum> {
    (
        ident_strategy(),
        prop::collection::vec(
            (ident_strategy(), 0..1000u32, prop::option::of(".*")).prop_map(
                |(name, idx, description)| EnumValue {
                    name,
                    idx,
                    description,
                },
            ),
            1..8,
        ),
    )
        .prop_map(|(name, values)| Enum { name, values })
}

fn protocol_strategy() -> impl Strategy<Value = Protocol> {
    (
        ident_strategy(),
        1..10u32,
        prop::collection::vec(enum_strategy(), 0..4),
        prop::option::of(".*"),
    )
        .prop_flat_map(|(proto_name, version, enums, copyright)| {
            let enum_names: Vec<String> = enums.iter().map(|e| e.name.clone()).collect();

            let obj_count = 1..6usize;
            (
                Just(proto_name),
                Just(version),
                Just(enums),
                Just(copyright),
                Just(enum_names),
                prop::collection::vec(ident_strategy(), obj_count),
            )
                .prop_flat_map(
                    |(proto_name, version, enums, copyright, enum_names, obj_names)| {
                        let mut unique_names = Vec::new();
                        for (i, name) in obj_names.iter().enumerate() {
                            let deduped = if unique_names.contains(name) {
                                format!("{name}_{i}")
                            } else {
                                name.clone()
                            };
                            unique_names.push(deduped);
                        }

                        let objects: Vec<_> = unique_names
                            .iter()
                            .map(|name| {
                                let en = enum_names.clone();
                                let on = unique_names.clone();
                                let name = name.clone();
                                (
                                    Just(name),
                                    1..5u32,
                                    prop::collection::vec(
                                        method_strategy(en.clone(), on.clone()),
                                        0..5,
                                    ),
                                    prop::collection::vec(method_strategy(en, on), 0..5),
                                    description_strategy(),
                                )
                                    .prop_map(
                                        |(name, obj_version, c2s, s2c, description)| Object {
                                            name,
                                            version: obj_version,
                                            c2s,
                                            s2c,
                                            description,
                                        },
                                    )
                            })
                            .collect();

                        (
                            Just(proto_name),
                            Just(version),
                            objects,
                            Just(enums),
                            Just(copyright),
                        )
                    },
                )
                .prop_map(|(name, version, objects, enums, copyright)| Protocol {
                    name,
                    version,
                    objects,
                    enums,
                    copyright,
                })
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn generate_all_does_not_panic(protocol in protocol_strategy()) {
        let _ = generate::generate(&protocol, Targets::ALL, &[]);
    }

    #[test]
    fn generate_client_only_does_not_panic(protocol in protocol_strategy()) {
        let _ = generate::generate(&protocol, Targets::CLIENT, &[]);
    }

    #[test]
    fn generate_server_only_does_not_panic(protocol in protocol_strategy()) {
        let _ = generate::generate(&protocol, Targets::SERVER, &[]);
    }

    #[test]
    fn generate_output_contains_protocol_modules(protocol in protocol_strategy()) {
        let code = generate::generate(&protocol, Targets::ALL, &[]);
        prop_assert!(code.contains("pub mod client"), "missing client module");
        prop_assert!(code.contains("pub mod server"), "missing server module");
        prop_assert!(code.contains("mod spec"), "missing spec module");
    }

    #[test]
    fn generate_output_contains_all_objects(protocol in protocol_strategy()) {
        let code = generate::generate(&protocol, Targets::ALL, &[]);
        for obj in &protocol.objects {
            let plain = format!("pub mod {}", obj.name);
            let raw = format!("pub mod r#{}", obj.name);
            prop_assert!(
                code.contains(&plain) || code.contains(&raw),
                "missing object module: {}",
                obj.name
            );
        }
    }

    #[test]
    fn generate_output_contains_all_enums(protocol in protocol_strategy()) {
        let code = generate::generate(&protocol, Targets::ALL, &[]);
        for e in &protocol.enums {
            let pascal = e.name.split('_')
                .map(|part| {
                    let mut c = part.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    }
                })
                .collect::<String>();
            prop_assert!(
                code.contains(&format!("pub enum {pascal}")),
                "missing enum: {pascal}"
            );
        }
    }

    #[test]
    fn generate_with_type_attributes_does_not_panic(protocol in protocol_strategy()) {
        let attrs = vec![
            (".".to_string(), "#[derive(Clone)]".to_string()),
        ];
        let _ = generate::generate(&protocol, Targets::ALL, &attrs);
    }
}
