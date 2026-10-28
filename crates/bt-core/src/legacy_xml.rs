//! Portable subset of the legacy XmlSerializer contracts used by scalar,
//! hotkey and placement settings. Never reads files or resolves external XML.
//!
//! The original deserializes through a streaming `XmlTextReader` with
//! significant-whitespace handling, so this module models that cursor rather
//! than building a tree: reading stops as soon as the value is complete, and
//! anything after that point (even malformed XML) is never inspected. Primitive
//! roots use `ReadElementString`; class members use
//! `ReadElementContentAsString`. Both behaviours are pinned by fixtures.
use crate::{models::HotkeyData, numbers::IntegerFormat};
use quick_xml::{
    NsReader, XmlVersion,
    events::{BytesStart, Event},
    name::ResolveResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use thiserror::Error;

mod keys {
    include!("data/dotnet_keys.rs");
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub enum Kind {
    Bool,
    I32,
    U32,
    F32,
    F64,
    String,
    Hotkey,
    Placement,
    OptionalPlacement,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPlacement {
    pub length: u32,
    pub flags: u32,
    pub show_cmd: u32,
    pub pt_min_position: Point,
    pub pt_max_position: Point,
    pub rc_normal_position: Rect,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    I32(i32),
    U32(u32),
    F32(f32),
    F64(f64),
    String(Option<String>),
    Hotkey(Option<HotkeyData>),
    Placement(Option<WindowPlacement>),
}
impl Value {
    /// JSON projection matches the reference's Newtonsoft scalar formatting.
    pub fn to_json(&self) -> Json {
        match self {
            Self::Bool(v) => json!(v),
            Self::I32(v) => json!(v),
            Self::U32(v) => json!(v),
            Self::String(v) => json!(v),
            Self::Hotkey(v) => json!(v),
            Self::Placement(v) => json!(v),
            Self::F32(v) if v.is_finite() => {
                serde_json::from_str(&serde_json::to_string(v).expect("finite f32"))
                    .expect("float JSON")
            }
            Self::F32(v) => float_json(f64::from(*v)),
            Self::F64(v) => float_json(*v),
        }
    }
}
fn float_json(value: f64) -> Json {
    if value.is_nan() {
        json!("NaN")
    } else if value == f64::INFINITY {
        json!("Infinity")
    } else if value == f64::NEG_INFINITY {
        json!("-Infinity")
    } else {
        json!(value)
    }
}
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("malformed legacy XML")]
    Xml,
    #[error("unexpected legacy XML root")]
    Root,
    #[error("invalid legacy XML value")]
    Value,
}

/// Only the 2001 instance namespace marks nil; older ones are captured as ignored.
const XSI: &[u8] = b"http://www.w3.org/2001/XMLSchema-instance";

#[derive(Debug)]
struct Element {
    name: String,
    namespace: Vec<u8>,
    empty: bool,
    nil: Option<String>,
}
/// Node kinds an `XmlTextReader` reports, reduced to what the readers need.
#[derive(Debug)]
enum Node {
    Start,
    Element(Element),
    End,
    /// Text, CDATA or significant whitespace (`whitespace` marks the last).
    Text {
        value: String,
        whitespace: bool,
    },
    /// Comment, processing instruction, XML declaration or document type.
    Other,
    Eof,
}
/// One parser event. Text carries whether it came from a character reference,
/// which never forms (insignificant) whitespace.
enum Raw {
    /// Element and its effective xml:space; open elements push it when consumed.
    Element(Element, Option<bool>),
    End,
    Text(String, bool),
    CData(String),
    Decl,
    DocType,
    Other,
    Eof,
}

struct Cursor<'a> {
    reader: NsReader<&'a [u8]>,
    xml: &'a str,
    checked: usize,
    pending: Option<Raw>,
    /// xml:space="preserve" state of each open element.
    preserve: Vec<bool>,
    root_seen: bool,
    doctype_seen: bool,
    events: usize,
    node: Node,
}
fn namespace(value: ResolveResult<'_>) -> Result<Vec<u8>, Error> {
    match value {
        ResolveResult::Unbound => Ok(vec![]),
        ResolveResult::Bound(value) => Ok(value.as_ref().to_vec()),
        ResolveResult::Unknown(_) => Err(Error::Xml),
    }
}
fn xml_char(c: char) -> bool {
    matches!(c,'\t'|'\r'|'\n'|'\u{20}'..='\u{d7ff}'|'\u{e000}'..='\u{fffd}'|'\u{10000}'..='\u{10ffff}')
}
fn xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}
impl<'a> Cursor<'a> {
    fn new(xml: &'a str) -> Self {
        let mut reader = NsReader::from_str(xml);
        reader.config_mut().check_comments = true;
        Self {
            reader,
            xml,
            checked: 0,
            pending: None,
            preserve: vec![],
            root_seen: false,
            doctype_seen: false,
            events: 0,
            node: Node::Start,
        }
    }
    fn element(&self, start: &BytesStart<'_>, empty: bool) -> Result<(Element, bool), Error> {
        let (ns, name) = self.reader.resolver().resolve_element(start.name());
        let namespace = namespace(ns)?;
        let name = std::str::from_utf8(name.as_ref())
            .map_err(|_| Error::Xml)?
            .to_owned();
        let mut nil = None;
        let mut preserve = self.preserve.last().copied().unwrap_or(false);
        for attribute in start.attributes() {
            let attribute = attribute.map_err(|_| Error::Xml)?;
            let key = attribute.key.as_ref();
            if key == b"xmlns" || key.starts_with(b"xmlns:") {
                continue;
            }
            let value = attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, self.reader.decoder())
                .map_err(|_| Error::Xml)?;
            if key == b"xml:space" {
                preserve = match value.as_ref() {
                    "preserve" => true,
                    "default" => false,
                    _ => return Err(Error::Xml),
                };
                continue;
            }
            let (ns, local) = self.reader.resolver().resolve_attribute(attribute.key);
            if self::namespace(ns)? == XSI && local.as_ref() == b"nil" {
                nil = Some(value.into_owned());
            }
        }
        let element = Element {
            name,
            namespace,
            empty,
            nil,
        };
        Ok((element, preserve))
    }
    /// Reads one parser event, validating the characters it consumed so that
    /// input after the point the original stops reading is never rejected.
    fn raw(&mut self) -> Result<Raw, Error> {
        if let Some(raw) = self.pending.take() {
            return Ok(raw);
        }
        let event = self.reader.read_event().map_err(|_| Error::Xml)?;
        let end = usize::try_from(self.reader.buffer_position()).map_err(|_| Error::Xml)?;
        if !self.xml[self.checked..end].chars().all(xml_char) {
            return Err(Error::Xml);
        }
        self.checked = end;
        Ok(match event {
            Event::Start(start) => {
                let (element, preserve) = self.element(&start, false)?;
                Raw::Element(element, Some(preserve))
            }
            Event::Empty(start) => Raw::Element(self.element(&start, true)?.0, None),
            Event::End(_) => Raw::End,
            Event::Text(text) => Raw::Text(
                text.xml10_content().map_err(|_| Error::Xml)?.into_owned(),
                false,
            ),
            Event::CData(text) => {
                Raw::CData(text.xml10_content().map_err(|_| Error::Xml)?.into_owned())
            }
            Event::GeneralRef(reference) => {
                if let Some(c) = reference.resolve_char_ref().map_err(|_| Error::Xml)? {
                    if !xml_char(c) {
                        return Err(Error::Xml);
                    }
                    Raw::Text(c.to_string(), true)
                } else {
                    // Other entities surface as EntityReference nodes, which no
                    // reader here accepts; undeclared ones are parse errors.
                    let name = reference.decode().map_err(|_| Error::Xml)?;
                    let text =
                        quick_xml::escape::resolve_predefined_entity(&name).ok_or(Error::Xml)?;
                    Raw::Text(text.to_owned(), false)
                }
            }
            Event::Decl(_) => Raw::Decl,
            Event::DocType(_) => Raw::DocType,
            Event::PI(_) | Event::Comment(_) => Raw::Other,
            Event::Eof => Raw::Eof,
        })
    }
    fn depth(&self) -> usize {
        self.preserve.len()
    }
    /// `XmlReader.Read` with `WhitespaceHandling.Significant`.
    fn read(&mut self) -> Result<(), Error> {
        loop {
            let first = self.events == 0;
            self.events += 1;
            let top = self.depth() == 0;
            let raw = self.raw()?;
            self.node = match raw {
                Raw::Element(element, preserve) => {
                    if top {
                        if self.root_seen {
                            return Err(Error::Xml);
                        }
                        self.root_seen = true;
                    }
                    self.preserve.extend(preserve);
                    Node::Element(element)
                }
                Raw::End => {
                    self.preserve.pop();
                    Node::End
                }
                Raw::Text(text, from_reference) => {
                    let preserve = self.preserve.last().copied().unwrap_or(false);
                    let (value, from_reference) = self.merge(text, from_reference)?;
                    let whitespace = !from_reference && value.chars().all(xml_space);
                    if top && !whitespace {
                        return Err(Error::Xml);
                    }
                    if top || whitespace && !preserve {
                        continue;
                    }
                    Node::Text { value, whitespace }
                }
                Raw::CData(_) if top => return Err(Error::Xml),
                Raw::CData(value) => Node::Text {
                    value,
                    whitespace: false,
                },
                Raw::Decl if !first => return Err(Error::Xml),
                Raw::DocType if !top || self.root_seen || self.doctype_seen => {
                    return Err(Error::Xml);
                }
                Raw::DocType => {
                    self.doctype_seen = true;
                    Node::Other
                }
                Raw::Decl | Raw::Other => Node::Other,
                Raw::Eof if !top || !self.root_seen => return Err(Error::Xml),
                Raw::Eof => Node::Eof,
            };
            return Ok(());
        }
    }
    /// Adjacent text and entity references form one reader text node.
    fn merge(&mut self, mut text: String, mut reference: bool) -> Result<(String, bool), Error> {
        loop {
            match self.raw()? {
                Raw::Text(more, more_reference) => {
                    text.push_str(&more);
                    reference |= more_reference;
                }
                other => {
                    self.pending = Some(other);
                    return Ok((text, reference));
                }
            }
        }
    }
    fn move_to_content(&mut self) -> Result<(), Error> {
        while matches!(
            self.node,
            Node::Start
                | Node::Other
                | Node::Text {
                    whitespace: true,
                    ..
                }
        ) {
            self.read()?;
        }
        Ok(())
    }
    fn current(&self) -> Option<&Element> {
        match &self.node {
            Node::Element(element) => Some(element),
            _ => None,
        }
    }
    fn skip(&mut self) -> Result<(), Error> {
        if self.current().is_some_and(|e| !e.empty) {
            let depth = self.depth() - 1;
            loop {
                self.read()?;
                if matches!(self.node, Node::End) && self.depth() == depth {
                    break;
                }
            }
        }
        self.read()
    }
    fn read_end_element(&mut self) -> Result<(), Error> {
        self.move_to_content()?;
        if !matches!(self.node, Node::End) {
            return Err(Error::Xml);
        }
        self.read()
    }
    /// `XmlSerializationReader.ReadNull`: consumes a nil element's content.
    fn read_null(&mut self) -> Result<bool, Error> {
        let element = self.current().ok_or(Error::Xml)?;
        if !element.nil.as_deref().map_or(Ok(false), boolean)? {
            return Ok(false);
        }
        let empty = element.empty;
        self.read()?;
        if !empty {
            while !matches!(self.node, Node::End) {
                self.skip()?;
            }
            self.read_end_element()?;
        }
        Ok(true)
    }
    /// Legacy `XmlReader.ReadString`: only leading textual nodes, descending
    /// into a first child element.
    fn read_string(&mut self) -> Result<String, Error> {
        if let Some(element) = self.current() {
            if element.empty {
                return Ok(String::new());
            }
            self.read()?;
            if matches!(self.node, Node::End) {
                return Ok(String::new());
            }
        }
        let mut result = String::new();
        while let Node::Text { value, .. } = &self.node {
            result.push_str(value);
            self.read()?;
        }
        Ok(result)
    }
    /// `XmlReader.ReadElementString` (primitive document roots).
    fn element_string(&mut self) -> Result<String, Error> {
        self.move_to_content()?;
        let empty = self.current().ok_or(Error::Xml)?.empty;
        self.read()?;
        if empty {
            return Ok(String::new());
        }
        let result = self.read_string()?;
        if !matches!(self.node, Node::End) {
            return Err(Error::Xml);
        }
        self.read()?;
        Ok(result)
    }
    /// `XmlReader.ReadElementContentAsString` (class members): comments and
    /// processing instructions are skipped, child elements are rejected.
    fn element_content(&mut self) -> Result<String, Error> {
        let empty = self.current().ok_or(Error::Xml)?.empty;
        self.read()?;
        let mut result = String::new();
        if empty {
            return Ok(result);
        }
        loop {
            match &self.node {
                Node::Text { value, .. } => result.push_str(value),
                Node::Other => {}
                Node::End => break,
                _ => return Err(Error::Xml),
            }
            self.read()?;
        }
        self.read()?;
        Ok(result)
    }
    /// Generated class reader loop: the first matching element per member
    /// wins; everything else, including repeats, is skipped as unknown.
    fn members(
        &mut self,
        names: &[&str],
        mut member: impl FnMut(&mut Self, usize) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self.current().ok_or(Error::Xml)?.empty {
            return self.read();
        }
        self.read()?;
        self.move_to_content()?;
        let mut seen = vec![false; names.len()];
        while !matches!(self.node, Node::End | Node::Eof) {
            let index = self.current().and_then(|e| {
                names
                    .iter()
                    .position(|n| e.namespace.is_empty() && e.name == *n)
                    .filter(|&i| !seen[i])
            });
            match index {
                Some(i) => {
                    seen[i] = true;
                    member(self, i)?;
                }
                None => self.skip()?,
            }
            self.move_to_content()?;
        }
        self.read_end_element()
    }
}
fn boolean(value: &str) -> Result<bool, Error> {
    match value.trim_matches(xml_space) {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(Error::Value),
    }
}
fn i32_value(text: &str) -> Result<i32, Error> {
    IntegerFormat::INVARIANT.parse_i32(text).ok_or(Error::Value)
}
fn u32_value(text: &str) -> Result<u32, Error> {
    let text = text.trim_matches(xml_space);
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Value);
    }
    text.parse().map_err(|_| Error::Value)
}
fn floating(text: &str) -> Result<f64, Error> {
    let text = text.trim_matches(xml_space);
    match text {
        "INF" => Ok(f64::INFINITY),
        "-INF" => Ok(f64::NEG_INFINITY),
        _ => {
            if text
                .trim_start_matches(['+', '-'])
                .eq_ignore_ascii_case("inf")
            {
                return Err(Error::Value);
            }
            let value: f64 = text.parse().map_err(|_| Error::Value)?;
            Ok(if value.is_nan() {
                crate::newtonsoft::DOTNET_NAN
            } else {
                value
            })
        }
    }
}
/// `[Flags]` enum text: whitespace-separated, case-sensitive member names.
fn flags(text: &str) -> Result<i32, Error> {
    let mut result = 0;
    for token in text.split_whitespace() {
        result |= keys::KEY_NAMES
            .iter()
            .find(|(name, _)| *name == token)
            .ok_or(Error::Value)?
            .1;
    }
    Ok(result)
}
fn hotkey(cursor: &mut Cursor<'_>) -> Result<HotkeyData, Error> {
    let mut value = HotkeyData::default();
    cursor.members(&["Modifiers", "Key"], |c, i| {
        let parsed = flags(&c.element_content()?)?;
        if i == 0 {
            value.modifiers = parsed;
        } else {
            value.key = parsed;
        }
        Ok(())
    })?;
    Ok(value)
}
fn coordinates<const N: usize>(
    cursor: &mut Cursor<'_>,
    names: &[&str; N],
) -> Result<[i32; N], Error> {
    let mut values = [0; N];
    cursor.members(names, |c, i| {
        values[i] = i32_value(&c.element_content()?)?;
        Ok(())
    })?;
    Ok(values)
}
/// Nested POINT/RECT structs ignore xsi:nil, as captured.
fn placement(cursor: &mut Cursor<'_>) -> Result<WindowPlacement, Error> {
    let mut value = WindowPlacement::default();
    let names = [
        "length",
        "flags",
        "showCmd",
        "ptMinPosition",
        "ptMaxPosition",
        "rcNormalPosition",
    ];
    cursor.members(&names, |c, i| {
        match i {
            0 => value.length = u32_value(&c.element_content()?)?,
            1 => value.flags = u32_value(&c.element_content()?)?,
            2 => value.show_cmd = u32_value(&c.element_content()?)?,
            3 | 4 => {
                let [x, y] = coordinates(c, &["x", "y"])?;
                let point = Point { x, y };
                if i == 3 {
                    value.pt_min_position = point;
                } else {
                    value.pt_max_position = point;
                }
            }
            _ => {
                let [left, top, right, bottom] =
                    coordinates(c, &["Left", "Top", "Right", "Bottom"])?;
                value.rc_normal_position = Rect {
                    left,
                    top,
                    right,
                    bottom,
                };
            }
        }
        Ok(())
    })?;
    Ok(value)
}

/// `[Flags]` Keys text as XmlSerializer writes it: a single defined name
/// for an exact value, else the Shift/Control/Alt flags followed by the key
/// code's name(s). Duplicate names (Return/Enter) use the first listed;
/// both read back to the same value in the original and in `decode`.
fn flags_text(value: i32) -> String {
    let exact = |v: i32| {
        keys::KEY_NAMES
            .iter()
            .find(|(_, n)| *n == v)
            .map(|(name, _)| *name)
    };
    if let Some(name) = exact(value) {
        return name.into();
    }
    let mut parts: Vec<&str> = vec![];
    let mut low = value & 0xffff;
    if let Some(name) = exact(low).filter(|_| low != 0) {
        parts.push(name);
        low = 0;
    }
    // No single name: decompose the key code like XmlSerializer's FromEnum.
    for (name, id) in keys::KEY_NAMES {
        if low == 0 {
            break;
        }
        if *id > 0 && *id <= 0xffff && id & low == *id {
            parts.push(name);
            low &= !id;
        }
    }
    let mut text: Vec<&str> = [
        ("Shift", 0x1_0000),
        ("Control", 0x2_0000),
        ("Alt", 0x4_0000),
    ]
    .into_iter()
    .filter(|(_, bit)| value & bit == *bit)
    .map(|(name, _)| name)
    .collect();
    text.extend(parts);
    if text.is_empty() {
        "None".into()
    } else {
        text.join(" ")
    }
}

/// The original's XmlSerializer text for a HotkeyData setting value
/// (`SettingsBag.Set<HotkeyData>`), `None` as the nil element.
pub fn encode_hotkey(value: Option<HotkeyData>) -> String {
    const HEADER: &str = r#"<?xml version="1.0" encoding="utf-16"?><HotkeyData xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema""#;
    match value {
        None => format!(r#"{HEADER} xsi:nil="true" />"#),
        Some(h) => format!(
            "{HEADER}><Modifiers>{}</Modifiers><Key>{}</Key></HotkeyData>",
            flags_text(h.modifiers),
            flags_text(h.key)
        ),
    }
}

/// Decodes one legacy XmlSerializer value with the original reader semantics.
pub fn decode(xml: &str, kind: Kind) -> Result<Value, Error> {
    let mut cursor = Cursor::new(xml);
    cursor.move_to_content()?;
    let root = match kind {
        Kind::Bool => "boolean",
        Kind::I32 => "int",
        Kind::U32 => "unsignedInt",
        Kind::F32 => "float",
        Kind::F64 => "double",
        Kind::String => "string",
        Kind::Hotkey => "HotkeyData",
        Kind::Placement | Kind::OptionalPlacement => "WINDOWPLACEMENT",
    };
    let element = cursor.current().ok_or(Error::Root)?;
    if element.name != root || !element.namespace.is_empty() {
        return Err(Error::Root);
    }
    let c = &mut cursor;
    Ok(match kind {
        Kind::Bool => Value::Bool(boolean(&c.element_string()?)?),
        Kind::I32 => Value::I32(i32_value(&c.element_string()?)?),
        Kind::U32 => Value::U32(u32_value(&c.element_string()?)?),
        Kind::F32 => Value::F32(floating(&c.element_string()?)? as f32),
        Kind::F64 => Value::F64(floating(&c.element_string()?)?),
        Kind::String => Value::String(if c.read_null()? {
            None
        } else {
            Some(c.element_string()?)
        }),
        Kind::Hotkey => Value::Hotkey(if c.read_null()? {
            None
        } else {
            Some(hotkey(c)?)
        }),
        Kind::OptionalPlacement if c.read_null()? => Value::Placement(None),
        Kind::OptionalPlacement | Kind::Placement => Value::Placement(Some(placement(c)?)),
    })
}
