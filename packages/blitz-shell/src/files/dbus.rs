//! PATCH(diaryx): the little of D-Bus a file picker needs, written here
//! rather than taken from a library: a connection to the session bus, its
//! EXTERNAL authentication, method calls and the signals they lead to, and
//! the wire format of the types the desktop portal's FileChooser uses.
//!
//! The protocol is the D-Bus specification's
//! (<https://dbus.freedesktop.org/doc/dbus-specification.html>): messages in
//! little-endian, each a fixed header, an array of header fields and a body,
//! every value aligned to its type's boundary from the message's start.
//! Only blocking reads and writes, on the picker's own thread.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;

/// A D-Bus value, as the picker writes and reads them.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(u8),
    Bool(bool),
    U32(u32),
    Str(String),
    Path(String),
    Sig(String),
    /// The elements' signature (for an empty array's), and the elements.
    Array(String, Vec<Value>),
    Struct(Vec<Value>),
    DictEntry(Box<Value>, Box<Value>),
    Variant(Box<Value>),
}

impl Value {
    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(s.into())
    }

    /// `a{sv}` from pairs.
    pub fn dict(pairs: Vec<(&str, Value)>) -> Value {
        Value::Array(
            "{sv}".into(),
            pairs
                .into_iter()
                .map(|(k, v)| {
                    Value::DictEntry(
                        Box::new(Value::str(k)),
                        Box::new(Value::Variant(Box::new(v))),
                    )
                })
                .collect(),
        )
    }

    pub fn signature(&self) -> String {
        match self {
            Value::Byte(_) => "y".into(),
            Value::Bool(_) => "b".into(),
            Value::U32(_) => "u".into(),
            Value::Str(_) => "s".into(),
            Value::Path(_) => "o".into(),
            Value::Sig(_) => "g".into(),
            Value::Array(elem, _) => format!("a{elem}"),
            Value::Struct(fields) => {
                format!(
                    "({})",
                    fields.iter().map(Value::signature).collect::<String>()
                )
            }
            Value::DictEntry(k, v) => format!("{{{}{}}}", k.signature(), v.signature()),
            Value::Variant(_) => "v".into(),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Path(s) | Value::Sig(s) => Some(s),
            Value::Variant(v) => v.as_str(),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Value::U32(n) => Some(*n),
            Value::Variant(v) => v.as_u32(),
            _ => None,
        }
    }

    /// An `a{sv}`'s value for `key`, unwrapped from its variant.
    pub fn get(&self, key: &str) -> Option<&Value> {
        let Value::Array(_, entries) = self else {
            return None;
        };
        entries.iter().find_map(|e| match e {
            Value::DictEntry(k, v) if k.as_str() == Some(key) => match &**v {
                Value::Variant(inner) => Some(&**inner),
                other => Some(other),
            },
            _ => None,
        })
    }

    /// An array's elements.
    pub fn items(&self) -> &[Value] {
        match self {
            Value::Array(_, items) => items,
            Value::Variant(v) => v.items(),
            _ => &[],
        }
    }
}

/// The alignment of a type, by its signature's first character.
fn alignment(sig: u8) -> usize {
    match sig {
        b'y' | b'g' | b'v' => 1,
        b'b' | b'u' | b'i' | b's' | b'o' | b'a' => 4,
        b'(' | b'{' | b't' | b'x' | b'd' => 8,
        b'n' | b'q' => 2,
        _ => 1,
    }
}

/// Marshals values, aligning from its own start: a body, which starts at an
/// eight-byte boundary of its message, or a message's header.
#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn pad(&mut self, align: usize) {
        while !self.0.len().is_multiple_of(align) {
            self.0.push(0);
        }
    }

    fn u32(&mut self, n: u32) {
        self.pad(4);
        self.0.extend_from_slice(&n.to_le_bytes());
    }

    fn value(&mut self, v: &Value) {
        match v {
            Value::Byte(b) => self.0.push(*b),
            Value::Bool(b) => self.u32(*b as u32),
            Value::U32(n) => self.u32(*n),
            Value::Str(s) | Value::Path(s) => {
                self.u32(s.len() as u32);
                self.0.extend_from_slice(s.as_bytes());
                self.0.push(0);
            }
            Value::Sig(s) => {
                self.0.push(s.len() as u8);
                self.0.extend_from_slice(s.as_bytes());
                self.0.push(0);
            }
            Value::Array(elem, items) => {
                self.pad(4);
                let len_at = self.0.len();
                self.0.extend_from_slice(&[0; 4]);
                // The length counts from after the padding to the first
                // element, even for an empty array.
                self.pad(alignment(elem.as_bytes()[0]));
                let start = self.0.len();
                for item in items {
                    self.value(item);
                }
                let len = (self.0.len() - start) as u32;
                self.0[len_at..len_at + 4].copy_from_slice(&len.to_le_bytes());
            }
            Value::Struct(fields) => {
                self.pad(8);
                for f in fields {
                    self.value(f);
                }
            }
            Value::DictEntry(k, v) => {
                self.pad(8);
                self.value(k);
                self.value(v);
            }
            Value::Variant(inner) => {
                self.value(&Value::Sig(inner.signature()));
                self.value(inner);
            }
        }
    }
}

/// Reads values by their signature, aligning from `base` (the offset of
/// the buffer's start within its message).
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    base: usize,
}

fn bad(m: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("D-Bus: {m}"))
}

impl Reader<'_> {
    fn align(&mut self, a: usize) -> io::Result<()> {
        while !(self.base + self.pos).is_multiple_of(a) {
            self.pos += 1;
        }
        if self.pos > self.buf.len() {
            return Err(bad("short message"));
        }
        Ok(())
    }

    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.buf.len());
        let end = end.ok_or_else(|| bad("short message"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u32(&mut self) -> io::Result<u32> {
        self.align(4)?;
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn string(&mut self, len: usize) -> io::Result<String> {
        let s =
            String::from_utf8(self.take(len)?.to_vec()).map_err(|_| bad("text is not UTF-8"))?;
        self.take(1)?; // its nul
        Ok(s)
    }

    /// One value of the complete type at the start of `sig`; the rest of
    /// `sig` is returned.
    fn value<'s>(&mut self, sig: &'s str) -> io::Result<(Value, &'s str)> {
        let c = *sig
            .as_bytes()
            .first()
            .ok_or_else(|| bad("empty signature"))?;
        let rest = &sig[1..];
        Ok(match c {
            b'y' => (Value::Byte(self.take(1)?[0]), rest),
            b'b' => (Value::Bool(self.u32()? != 0), rest),
            b'u' | b'i' => (Value::U32(self.u32()?), rest),
            b'n' | b'q' => {
                self.align(2)?;
                let b = self.take(2)?;
                (Value::U32(u16::from_le_bytes([b[0], b[1]]) as u32), rest)
            }
            b't' | b'x' | b'd' => {
                self.align(8)?;
                self.take(8)?;
                (Value::U32(0), rest)
            }
            b's' | b'o' => {
                let len = self.u32()? as usize;
                let s = self.string(len)?;
                (
                    if c == b's' {
                        Value::Str(s)
                    } else {
                        Value::Path(s)
                    },
                    rest,
                )
            }
            b'g' => {
                let len = self.take(1)?[0] as usize;
                (Value::Sig(self.string(len)?), rest)
            }
            b'v' => {
                let len = self.take(1)?[0] as usize;
                let inner = self.string(len)?;
                let (v, left) = self.value(&inner)?;
                if !left.is_empty() {
                    return Err(bad("a variant of more than one type"));
                }
                (Value::Variant(Box::new(v)), rest)
            }
            b'a' => {
                let len = self.u32()? as usize;
                let elem_end = single_type_len(rest)?;
                let elem = &rest[..elem_end];
                self.align(alignment(elem.as_bytes()[0]))?;
                let end = self.pos + len;
                let mut items = Vec::new();
                while self.pos < end {
                    items.push(self.value(elem)?.0);
                }
                (Value::Array(elem.into(), items), &rest[elem_end..])
            }
            b'(' | b'{' => {
                self.align(8)?;
                let close = if c == b'(' { b')' } else { b'}' };
                let mut fields = Vec::new();
                let mut inner = rest;
                while inner.as_bytes().first() != Some(&close) {
                    let (v, left) = self.value(inner)?;
                    fields.push(v);
                    inner = left;
                }
                let rest = &inner[1..];
                if c == b'{' {
                    let mut it = fields.into_iter();
                    let (k, v) = (it.next(), it.next());
                    let (Some(k), Some(v)) = (k, v) else {
                        return Err(bad("a dict entry without a key and a value"));
                    };
                    (Value::DictEntry(Box::new(k), Box::new(v)), rest)
                } else {
                    (Value::Struct(fields), rest)
                }
            }
            _ => return Err(bad("a type this reader does not know")),
        })
    }
}

/// The length of the single complete type at the start of `sig`.
fn single_type_len(sig: &str) -> io::Result<usize> {
    let b = sig.as_bytes();
    match b.first() {
        Some(b'a') => Ok(1 + single_type_len(&sig[1..])?),
        Some(&open @ (b'(' | b'{')) => {
            let close = if open == b'(' { b')' } else { b'}' };
            let mut depth = 0;
            for (i, &c) in b.iter().enumerate() {
                if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(i + 1);
                    }
                }
            }
            Err(bad("an unclosed signature"))
        }
        Some(_) => Ok(1),
        None => Err(bad("empty signature")),
    }
}

/// A message, as received.
#[derive(Debug)]
pub struct Message {
    /// 1 a call, 2 a return, 3 an error, 4 a signal.
    pub kind: u8,
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub error_name: Option<String>,
    pub reply_serial: Option<u32>,
    pub body: Vec<Value>,
}

pub const METHOD_RETURN: u8 = 2;
pub const ERROR: u8 = 3;
pub const SIGNAL: u8 = 4;

/// A call to make: where, what, and its arguments.
pub struct Call<'a> {
    pub destination: &'a str,
    pub path: &'a str,
    pub interface: &'a str,
    pub member: &'a str,
    pub args: Vec<Value>,
}

/// One whole message, to send.
fn encode(serial: u32, call: &Call) -> Vec<u8> {
    let mut body = Writer::default();
    for a in &call.args {
        body.value(a);
    }
    let sig: String = call.args.iter().map(Value::signature).collect();
    let field =
        |code: u8, v: Value| Value::Struct(vec![Value::Byte(code), Value::Variant(Box::new(v))]);
    let mut fields = vec![
        field(1, Value::Path(call.path.into())),
        field(2, Value::str(call.interface)),
        field(3, Value::str(call.member)),
        field(6, Value::str(call.destination)),
    ];
    if !sig.is_empty() {
        fields.push(field(8, Value::Sig(sig)));
    }
    let mut msg = Writer::default();
    msg.0.extend_from_slice(&[b'l', 1, 0, 1]);
    msg.0
        .extend_from_slice(&(body.0.len() as u32).to_le_bytes());
    msg.0.extend_from_slice(&serial.to_le_bytes());
    msg.value(&Value::Array("(yv)".into(), fields));
    msg.pad(8);
    msg.0.extend_from_slice(&body.0);
    msg.0
}

/// One message, from its whole bytes.
fn decode(bytes: &[u8]) -> io::Result<Message> {
    if bytes.first() != Some(&b'l') {
        return Err(bad("a big-endian message, which this reader does not read"));
    }
    let kind = bytes[1];
    let body_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let mut r = Reader {
        buf: bytes,
        pos: 12,
        base: 0,
    };
    let (fields, _) = r.value("a(yv)")?;
    r.align(8)?;
    let body_start = r.pos;
    let mut m = Message {
        kind,
        path: None,
        interface: None,
        member: None,
        error_name: None,
        reply_serial: None,
        body: Vec::new(),
    };
    let mut sig = String::new();
    for f in fields.items() {
        let Value::Struct(f) = f else { continue };
        let (Some(Value::Byte(code)), Some(v)) = (f.first(), f.get(1)) else {
            continue;
        };
        match code {
            1 => m.path = v.as_str().map(Into::into),
            2 => m.interface = v.as_str().map(Into::into),
            3 => m.member = v.as_str().map(Into::into),
            4 => m.error_name = v.as_str().map(Into::into),
            5 => m.reply_serial = v.as_u32(),
            8 => sig = v.as_str().unwrap_or_default().into(),
            _ => {}
        }
    }
    let body = bytes
        .get(body_start..body_start + body_len)
        .ok_or_else(|| bad("short body"))?;
    let mut r = Reader {
        buf: body,
        pos: 0,
        base: 0,
    };
    let mut rest = sig.as_str();
    while !rest.is_empty() {
        let (v, left) = r.value(rest)?;
        m.body.push(v);
        rest = left;
    }
    Ok(m)
}

/// A connection to the session bus.
pub struct Bus {
    stream: BufReader<UnixStream>,
    serial: u32,
    /// Our name on the bus (`:1.42`), which a portal's request path is made
    /// from.
    pub unique_name: String,
}

impl Bus {
    /// Connect to the session bus `DBUS_SESSION_BUS_ADDRESS` names, and
    /// introduce ourselves.
    pub fn session() -> io::Result<Bus> {
        let address = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "no session bus"))?;
        let stream = address
            .split(';')
            .find_map(|a| connect(a).ok())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "cannot reach the session bus")
            })?;
        let mut bus = Bus {
            stream: BufReader::new(stream),
            serial: 0,
            unique_name: String::new(),
        };
        bus.authenticate()?;
        let hello = bus.call(&Call {
            destination: "org.freedesktop.DBus",
            path: "/org/freedesktop/DBus",
            interface: "org.freedesktop.DBus",
            member: "Hello",
            args: vec![],
        })?;
        bus.unique_name = hello
            .body
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| bad("Hello answered no name"))?
            .into();
        Ok(bus)
    }

    /// EXTERNAL: the bus knows us by our socket's credentials, and we name
    /// our user id to it.
    fn authenticate(&mut self) -> io::Result<()> {
        let uid = unsafe { libc_getuid() };
        let hex: String = uid
            .to_string()
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect();
        let s = self.stream.get_mut();
        s.write_all(b"\0")?;
        s.write_all(format!("AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let mut line = String::new();
        self.stream.read_line(&mut line)?;
        if !line.starts_with("OK ") {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("the session bus refused us: {}", line.trim()),
            ));
        }
        self.stream.get_mut().write_all(b"BEGIN\r\n")
    }

    /// Send `call`, and return its serial.
    pub fn send(&mut self, call: &Call) -> io::Result<u32> {
        self.serial += 1;
        let bytes = encode(self.serial, call);
        self.stream.get_mut().write_all(&bytes)?;
        Ok(self.serial)
    }

    /// The next message from the bus.
    pub fn receive(&mut self) -> io::Result<Message> {
        let mut fixed = [0u8; 16];
        self.stream.read_exact(&mut fixed)?;
        let body_len = u32::from_le_bytes([fixed[4], fixed[5], fixed[6], fixed[7]]) as usize;
        let fields_len = u32::from_le_bytes([fixed[12], fixed[13], fixed[14], fixed[15]]) as usize;
        let header_len = (16 + fields_len).div_ceil(8) * 8;
        let total = header_len + body_len;
        if total > 128 * 1024 * 1024 {
            return Err(bad("a message too large to read"));
        }
        let mut bytes = fixed.to_vec();
        bytes.resize(total, 0);
        self.stream.read_exact(&mut bytes[16..])?;
        decode(&bytes)
    }

    /// Make `call` and wait for its answer, passing over anything else; an
    /// error answer is an error.
    pub fn call(&mut self, call: &Call) -> io::Result<Message> {
        let serial = self.send(call)?;
        loop {
            let m = self.receive()?;
            if m.reply_serial != Some(serial) {
                continue;
            }
            if m.kind == ERROR {
                let why = m.body.first().and_then(Value::as_str).unwrap_or_default();
                return Err(io::Error::other(format!(
                    "{}: {why}",
                    m.error_name.as_deref().unwrap_or("error")
                )));
            }
            return Ok(m);
        }
    }
}

fn connect(address: &str) -> io::Result<UnixStream> {
    let (transport, params) = address
        .split_once(':')
        .ok_or_else(|| bad("an address without a transport"))?;
    if transport != "unix" {
        return Err(bad("not a unix socket"));
    }
    for kv in params.split(',') {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        let v = unescape(v);
        match k {
            "path" => return UnixStream::connect(v),
            #[cfg(any(target_os = "linux", target_os = "android"))]
            "abstract" => {
                use std::os::linux::net::SocketAddrExt;
                let addr = std::os::unix::net::SocketAddr::from_abstract_name(v.as_bytes())?;
                return UnixStream::connect_addr(&addr);
            }
            _ => {}
        }
    }
    Err(bad("an address with no socket"))
}

/// An address's value with its `%xx` escapes undone.
fn unescape(v: &str) -> String {
    String::from_utf8_lossy(&percent_decode(v)).into_owned()
}

/// `s`'s bytes with its `%xx` escapes undone: a bus address's, a URI's.
pub fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(n) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(n);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_read_as_it_was_written() {
        let call = Call {
            destination: "org.freedesktop.portal.Desktop",
            path: "/org/freedesktop/portal/desktop",
            interface: "org.freedesktop.portal.FileChooser",
            member: "OpenFile",
            args: vec![
                Value::str(""),
                Value::str("Choose a file"),
                Value::dict(vec![
                    ("handle_token", Value::str("blitz1")),
                    ("multiple", Value::Bool(true)),
                    (
                        "filters",
                        Value::Array(
                            "(sa(us))".into(),
                            vec![Value::Struct(vec![
                                Value::str("Images"),
                                Value::Array(
                                    "(us)".into(),
                                    vec![Value::Struct(vec![Value::U32(1), Value::str("image/*")])],
                                ),
                            ])],
                        ),
                    ),
                    (
                        "current_folder",
                        Value::Array(
                            "y".into(),
                            b"/tmp\0".iter().map(|b| Value::Byte(*b)).collect(),
                        ),
                    ),
                    ("empty", Value::Array("s".into(), vec![])),
                ]),
            ],
        };
        let bytes = encode(7, &call);
        // The body starts at an eight-byte boundary; its end need not.
        assert_eq!(&bytes[..4], b"l\x01\x00\x01");
        let m = decode(&bytes).unwrap();
        assert_eq!(m.member.as_deref(), Some("OpenFile"));
        assert_eq!(m.path.as_deref(), Some("/org/freedesktop/portal/desktop"));
        assert_eq!(m.body, call.args);
        let options = &m.body[2];
        assert_eq!(
            options.get("handle_token").and_then(Value::as_str),
            Some("blitz1")
        );
        assert_eq!(options.get("multiple"), Some(&Value::Bool(true)));
        assert_eq!(options.get("empty").map(|v| v.items().len()), Some(0));
    }

    #[test]
    fn a_signature_is_split_into_its_types() {
        assert_eq!(single_type_len("a{sv}u").unwrap(), 5);
        assert_eq!(single_type_len("(sa(us))s").unwrap(), 8);
        assert_eq!(single_type_len("u").unwrap(), 1);
        assert!(single_type_len("(su").is_err());
    }

    #[test]
    fn an_address_is_unescaped() {
        assert_eq!(unescape("/run/user/1000/bus"), "/run/user/1000/bus");
        assert_eq!(unescape("/tmp/a%20b"), "/tmp/a b");
    }
}
