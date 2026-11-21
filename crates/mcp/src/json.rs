//! JSON as the MCP server reads and writes it, from RFC 8259: a value tree, a bounded reader
//! and a writer that puts a message on one line.
//!
//! The reader takes untrusted text: the size of a message, the depth of its nesting and the
//! number of members are bounded before anything is allocated for them.

use std::fmt::Write as _;

/// The longest message read, in bytes.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// The deepest nesting of arrays and objects read.
pub const MAX_DEPTH: usize = 64;
/// The most members of an array or an object, and of a whole message.
pub const MAX_MEMBERS: usize = 10_000;

/// A JSON value. Object members keep the order they came in; a repeated name keeps its first
/// value for [`Value::get`].
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonError {
    TooLarge,
    TooDeep,
    TooManyMembers,
    /// The text is not JSON; the byte where the reader stopped.
    Syntax(usize),
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(formatter, "the message is over {MAX_MESSAGE_BYTES} bytes"),
            Self::TooDeep => write!(formatter, "the message nests over {MAX_DEPTH} levels"),
            Self::TooManyMembers => write!(formatter, "the message has over {MAX_MEMBERS} members"),
            Self::Syntax(at) => write!(formatter, "the message is not JSON (byte {at})"),
        }
    }
}

impl std::error::Error for JsonError {}

impl Value {
    /// An object from its members.
    pub fn object<const N: usize>(members: [(&str, Value); N]) -> Self {
        Self::Object(
            members
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// The member `name` of an object.
    pub fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Self::Object(members) => members
                .iter()
                .find(|(member, _)| member == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The value on one line, as the stdio transport needs it.
    pub fn to_line(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            // JSON has no infinity nor NaN.
            Self::Number(number) if !number.is_finite() => out.push_str("null"),
            Self::Number(number) if number.fract() == 0.0 && number.abs() < 1e15 => {
                let _ = write!(out, "{}", *number as i64);
            }
            Self::Number(number) => {
                let _ = write!(out, "{number}");
            }
            Self::Text(text) => write_text(text, out),
            Self::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Self::Object(members) => {
                out.push('{');
                for (index, (name, value)) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_text(name, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_text(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Reads one JSON value that fills `text`, whitespace around it aside.
pub fn parse(text: &str) -> Result<Value, JsonError> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(JsonError::TooLarge);
    }
    let mut reader = Reader {
        bytes: text.as_bytes(),
        at: 0,
        members: 0,
    };
    let value = reader.value(0)?;
    reader.space();
    if reader.at != reader.bytes.len() {
        return Err(JsonError::Syntax(reader.at));
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    members: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn error(&self) -> JsonError {
        JsonError::Syntax(self.at)
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), JsonError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.error())
        }
    }

    fn word(&mut self, word: &str, value: Value) -> Result<Value, JsonError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error())
        }
    }

    fn count(&mut self) -> Result<(), JsonError> {
        self.members += 1;
        if self.members > MAX_MEMBERS {
            Err(JsonError::TooManyMembers)
        } else {
            Ok(())
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.space();
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.text().map(Value::Text),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.error()),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonError> {
        if depth > MAX_DEPTH {
            return Err(JsonError::TooDeep);
        }
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.count()?;
            items.push(self.value(depth)?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.error()),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        if depth > MAX_DEPTH {
            return Err(JsonError::TooDeep);
        }
        self.expect(b'{')?;
        let mut members = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.count()?;
            self.space();
            let name = self.text()?;
            self.space();
            self.expect(b':')?;
            let value = self.value(depth)?;
            members.push((name, value));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(members));
                }
                _ => return Err(self.error()),
            }
        }
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        let digits = |reader: &mut Self| {
            let from = reader.at;
            while matches!(reader.peek(), Some(b'0'..=b'9')) {
                reader.at += 1;
            }
            reader.at > from
        };
        if !digits(self) {
            return Err(self.error());
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !digits(self) {
                return Err(self.error());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !digits(self) {
                return Err(self.error());
            }
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .filter(|number| number.is_finite())
            .map(Value::Number)
            .ok_or(JsonError::Syntax(start))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self.bytes.get(self.at..self.at + 4).ok_or(self.error())?;
        let text = std::str::from_utf8(digits).map_err(|_| self.error())?;
        let value = u32::from_str_radix(text, 16).map_err(|_| self.error())?;
        self.at += 4;
        Ok(value)
    }

    fn text(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.at;
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.at += 1;
            }
            // The input is a `str`, and the run stops only at ASCII bytes.
            let run = std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| self.error())?;
            out.push_str(run);
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = self.peek().ok_or(self.error())?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                // A surrogate pair: the second half must follow.
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err(self.error());
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            out.push(char::from_u32(code).ok_or(self.error())?);
                        }
                        _ => return Err(self.error()),
                    }
                }
                _ => return Err(self.error()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_reads_back_as_it_was_written() {
        let text = r#" {"jsonrpc":"2.0","id":7,"params":{"name":"x\"y\\n\u00e9\ud83c\udfb5","list":[1.5,-2e3,true,null,[]]}} "#;
        let value = parse(text).unwrap_or(Value::Null);
        assert_eq!(value.get("id"), Some(&Value::Number(7.0)));
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        assert_eq!(
            params.get("name").and_then(Value::as_str),
            Some("x\"y\\né🎵")
        );
        let line = value.to_line();
        assert!(!line.contains('\n'));
        assert_eq!(parse(&line), Ok(value));
    }

    #[test]
    fn damaged_or_oversized_messages_are_refused() {
        for text in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "01x",
            "\"\\q\"",
            "\"\\ud800\"",
            "1 2",
            "nul",
        ] {
            assert!(parse(text).is_err(), "{text:?}");
        }
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(parse(&deep), Err(JsonError::TooDeep));
        let many = format!("[{}0]", "0,".repeat(MAX_MEMBERS));
        assert_eq!(parse(&many), Err(JsonError::TooManyMembers));
        let large = format!("\"{}\"", "a".repeat(MAX_MESSAGE_BYTES));
        assert_eq!(parse(&large), Err(JsonError::TooLarge));
    }

    #[test]
    fn control_characters_are_escaped_and_numbers_stay_finite() {
        let value = Value::Array(vec![
            Value::text("a\u{1}b\tc"),
            Value::Number(f64::NAN),
            Value::Number(3.0),
            Value::Number(0.25),
        ]);
        assert_eq!(value.to_line(), r#"["a\u0001b\tc",null,3,0.25]"#);
    }
}
