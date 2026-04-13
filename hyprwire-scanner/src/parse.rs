use quick_xml::Reader;
use quick_xml::events::Event;
use std::error::Error;

#[derive(Debug, Clone)]
pub struct Protocol {
    pub name: String,
    pub version: u32,
    pub objects: Vec<Object>,
    pub enums: Vec<Enum>,
    pub copyright: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub name: String,
    pub version: u32,
    pub c2s: Vec<Method>,
    pub s2c: Vec<Method>,
    pub description: Option<Description>,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
    pub args: Vec<Arg>,
    pub returns: Option<String>,
    pub destructor: bool,
    pub description: Option<Description>,
}

#[derive(Debug, Clone)]
pub struct Arg {
    pub name: String,
    pub arg_type: ArgType,
    pub interface: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Description {
    pub summary: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArgType {
    Varchar,
    Fd,
    Uint,
    Int,
    F32,
    Enum,
    ArrayVarchar,
    ArrayFd,
    ArrayUint,
    ArrayInt,
    ArrayF32,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub name: String,
    pub values: Vec<EnumValue>,
}

#[derive(Debug, Clone)]
pub struct EnumValue {
    pub name: String,
    pub idx: u32,
    pub description: Option<String>,
}

fn parse_arg_type(type_str: &str) -> ArgType {
    match type_str {
        "varchar" => ArgType::Varchar,
        "fd" => ArgType::Fd,
        "uint" => ArgType::Uint,
        "int" => ArgType::Int,
        "f32" => ArgType::F32,
        "enum" => ArgType::Enum,
        "array varchar" => ArgType::ArrayVarchar,
        "array fd" => ArgType::ArrayFd,
        "array uint" => ArgType::ArrayUint,
        "array int" => ArgType::ArrayInt,
        "array f32" => ArgType::ArrayF32,
        other => panic!("unknown arg type: {other}"),
    }
}

fn attr_str(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes()
        .filter_map(std::result::Result::ok)
        .find(|a| a.key.as_ref() == key)
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn attr_required(
    e: &quick_xml::events::BytesStart<'_>,
    key: &[u8],
) -> Result<String, Box<dyn Error>> {
    attr_str(e, key)
        .ok_or_else(|| format!("missing attribute '{}'", String::from_utf8_lossy(key)).into())
}

fn parse_method(
    reader: &mut Reader<&[u8]>,
    e: &quick_xml::events::BytesStart<'_>,
) -> Result<Method, Box<dyn Error>> {
    let name = attr_required(e, b"name")?;
    let destructor = attr_str(e, b"destructor").is_some_and(|v| v == "true");
    let mut args = Vec::new();
    let mut returns = None;
    let mut description = None;

    loop {
        match reader.read_event()? {
            Event::Empty(ref inner) => match inner.name().as_ref() {
                b"arg" => {
                    let arg_name = attr_required(inner, b"name")?;
                    let arg_type_str = attr_required(inner, b"type")?;
                    let interface = attr_str(inner, b"interface");
                    let summary = attr_str(inner, b"summary");
                    args.push(Arg {
                        name: arg_name,
                        arg_type: parse_arg_type(&arg_type_str),
                        interface,
                        summary,
                    });
                }
                b"returns" => {
                    returns = Some(attr_required(inner, b"iface")?);
                }
                _ => {}
            },
            Event::Start(ref inner) => match inner.name().as_ref() {
                b"description" => {
                    description = Some(parse_description(reader, inner)?);
                }
                b"arg" => {
                    let arg_name = attr_required(inner, b"name")?;
                    let arg_type_str = attr_required(inner, b"type")?;
                    let interface = attr_str(inner, b"interface");
                    let summary = attr_str(inner, b"summary");
                    reader.read_to_end(inner.name())?;
                    args.push(Arg {
                        name: arg_name,
                        arg_type: parse_arg_type(&arg_type_str),
                        interface,
                        summary,
                    });
                }
                _ => {
                    reader.read_to_end(inner.name())?;
                }
            },
            Event::End(_) => break,
            Event::Eof => return Err("unexpected EOF in method".into()),
            _ => {}
        }
    }

    Ok(Method {
        name,
        args,
        returns,
        destructor,
        description,
    })
}

fn read_text_to_end(
    reader: &mut Reader<&[u8]>,
    end: &quick_xml::events::BytesStart<'_>,
) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    loop {
        match reader.read_event()? {
            Event::Text(t) => {
                out.push_str(&t.unescape()?);
            }
            Event::CData(t) => {
                let cdata = t.into_inner();
                out.push_str(&String::from_utf8_lossy(cdata.as_ref()));
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::Eof => return Err("unexpected EOF in copyright".into()),
            _ => {}
        }
    }
    Ok(out)
}

fn parse_description(
    reader: &mut Reader<&[u8]>,
    e: &quick_xml::events::BytesStart<'_>,
) -> Result<Description, Box<dyn Error>> {
    let summary = attr_str(e, b"summary");
    let body = read_text_to_end(reader, e)?;
    let body = if body.trim().is_empty() {
        None
    } else {
        Some(body)
    };
    Ok(Description { summary, body })
}

pub fn parse_protocol(xml: &str) -> Result<Protocol, Box<dyn Error>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut protocol_name = String::new();
    let mut protocol_version = 0u32;
    let mut objects = Vec::new();
    let mut enums = Vec::new();
    let mut copyright = None;

    loop {
        match reader.read_event()? {
            Event::Start(ref e) => {
                let tag = e.name();
                match tag.as_ref() {
                    b"protocol" => {
                        protocol_name = attr_required(e, b"name")?;
                        protocol_version = attr_required(e, b"version")?.parse()?;
                    }
                    b"copyright" => {
                        let text = read_text_to_end(&mut reader, e)?;
                        if !text.trim().is_empty() {
                            copyright = Some(text);
                        }
                    }
                    b"object" => {
                        let obj_name = attr_required(e, b"name")?;
                        let obj_version: u32 = attr_required(e, b"version")?.parse()?;
                        let mut c2s = Vec::new();
                        let mut s2c = Vec::new();
                        let mut description = None;

                        loop {
                            match reader.read_event()? {
                                Event::Start(ref inner) => {
                                    let inner_tag = inner.name();
                                    match inner_tag.as_ref() {
                                        b"description" => {
                                            description =
                                                Some(parse_description(&mut reader, inner)?);
                                        }
                                        b"c2s" => {
                                            c2s.push(parse_method(&mut reader, inner)?);
                                        }
                                        b"s2c" => {
                                            s2c.push(parse_method(&mut reader, inner)?);
                                        }
                                        _ => {
                                            reader.read_to_end(inner_tag)?;
                                        }
                                    }
                                }
                                Event::End(_) => break,
                                Event::Eof => return Err("unexpected EOF in object".into()),
                                _ => {}
                            }
                        }

                        objects.push(Object {
                            name: obj_name,
                            version: obj_version,
                            c2s,
                            s2c,
                            description,
                        });
                    }
                    b"enum" => {
                        let enum_name = attr_required(e, b"name")?;
                        let mut values = Vec::new();

                        loop {
                            match reader.read_event()? {
                                Event::Empty(ref inner) | Event::Start(ref inner)
                                    if inner.name().as_ref() == b"value" =>
                                {
                                    let idx: u32 = attr_required(inner, b"idx")?.parse()?;
                                    let val_name = attr_required(inner, b"name")?;
                                    let description = attr_str(inner, b"description");
                                    values.push(EnumValue {
                                        name: val_name,
                                        idx,
                                        description,
                                    });
                                }
                                Event::End(_) => break,
                                Event::Eof => return Err("unexpected EOF in enum".into()),
                                _ => {}
                            }
                        }

                        enums.push(Enum {
                            name: enum_name,
                            values,
                        });
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(Protocol {
        name: protocol_name,
        version: protocol_version,
        objects,
        enums,
        copyright,
    })
}
