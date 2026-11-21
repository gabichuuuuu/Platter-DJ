//! A reader of the XML that playlist and collection files use: elements, their attributes and
//! their text, read as a flat list of events.
//!
//! It is not a general XML parser. It takes UTF-8 documents, skips the declaration, comments,
//! processing instructions and the document type, and decodes the five predefined entities
//! and character references. It checks that elements nest. Everything it reads is bounded:
//! the caller limits the size of the document, and the depth and the attributes of one element
//! are limited here. Specification: <https://www.w3.org/TR/xml/>, sections 2.4 to 2.8, 3.1
//! and 4.1, of which this is the subset those files use.

/// The deepest nesting read.
pub const MAX_DEPTH: usize = 64;
/// The most attributes of one element.
pub const MAX_ATTRIBUTES: usize = 64;

/// One thing the reader meets, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// An element opens; `empty` when it closes at once (`<a/>`).
    Start {
        name: String,
        attributes: Vec<(String, String)>,
        empty: bool,
    },
    End {
        name: String,
    },
    /// Text between tags, decoded, when it holds more than white space.
    Text(String),
}

/// Why a document cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XmlError {
    NotUtf8,
    /// The document breaks off or breaks the rules at this byte.
    Damaged(usize),
    TooDeep,
}

impl Event {
    /// The value of attribute `name` of a start, if it has one.
    pub fn attribute(&self, name: &str) -> Option<&str> {
        match self {
            Self::Start { attributes, .. } => attributes
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str()),
            _ => None,
        }
    }
}

/// Reads `bytes` into its events. A byte order mark at the start is skipped.
pub fn read(bytes: &[u8]) -> Result<Vec<Event>, XmlError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| XmlError::NotUtf8)?;
    let mut events = Vec::new();
    let mut open: Vec<String> = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let Some(tag) = rest.find('<') else {
            push_text(&mut events, rest, at)?;
            break;
        };
        if tag > 0 {
            push_text(&mut events, &rest[..tag], at)?;
        }
        at += tag;
        let rest = &text[at..];
        let skipped = [("<?", "?>"), ("<!--", "-->"), ("<![CDATA[", "]]>")]
            .into_iter()
            .find(|(opening, _)| rest.starts_with(opening));
        if let Some((opening, closing)) = skipped {
            let end = rest[opening.len()..]
                .find(closing)
                .ok_or(XmlError::Damaged(at))?;
            if opening == "<![CDATA[" {
                let data = &rest[opening.len()..opening.len() + end];
                if !data.trim().is_empty() {
                    events.push(Event::Text(data.to_owned()));
                }
            }
            at += opening.len() + end + closing.len();
            continue;
        }
        if rest.starts_with("<!") {
            // The document type, with an internal subset in brackets at most.
            let end = doctype_end(rest).ok_or(XmlError::Damaged(at))?;
            at += end;
            continue;
        }
        let end = tag_end(rest).ok_or(XmlError::Damaged(at))?;
        let inside = &rest[1..end];
        at += end + 1;
        if let Some(name) = inside.strip_prefix('/') {
            let name = name.trim_end();
            if open.pop().as_deref() != Some(name) {
                return Err(XmlError::Damaged(at));
            }
            events.push(Event::End {
                name: name.to_owned(),
            });
            continue;
        }
        let (inside, empty) = match inside.strip_suffix('/') {
            Some(inside) => (inside, true),
            None => (inside, false),
        };
        let (name, attributes) = start_tag(inside).ok_or(XmlError::Damaged(at))?;
        if !empty {
            if open.len() >= MAX_DEPTH {
                return Err(XmlError::TooDeep);
            }
            open.push(name.clone());
        }
        events.push(Event::Start {
            name,
            attributes,
            empty,
        });
    }
    if open.is_empty() {
        Ok(events)
    } else {
        Err(XmlError::Damaged(text.len()))
    }
}

fn push_text(events: &mut Vec<Event>, raw: &str, at: usize) -> Result<(), XmlError> {
    if raw.trim().is_empty() {
        return Ok(());
    }
    events.push(Event::Text(unescape(raw).ok_or(XmlError::Damaged(at))?));
    Ok(())
}

/// Where the `>` that ends the tag starting `rest` is, outside quoted values.
fn tag_end(rest: &str) -> Option<usize> {
    let mut quote = None;
    for (index, c) in rest.char_indices().skip(1) {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), c) if c == open => quote = None,
            (None, '>') => return Some(index),
            (None, '<') => return None,
            _ => {}
        }
    }
    None
}

/// The length of a `<!DOCTYPE ...>` that starts `rest`.
fn doctype_end(rest: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, c) in rest.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.checked_sub(1)?,
            '>' if depth == 0 => return Some(index + 1),
            _ => {}
        }
    }
    None
}

/// The name and the attributes of a start tag, without its brackets.
fn start_tag(inside: &str) -> Option<(String, Vec<(String, String)>)> {
    let inside = inside.trim_end();
    let name_end = inside
        .find(|c: char| c.is_whitespace())
        .unwrap_or(inside.len());
    let name = &inside[..name_end];
    if !is_name(name) {
        return None;
    }
    let mut attributes = Vec::new();
    let mut rest = inside[name_end..].trim_start();
    while !rest.is_empty() {
        if attributes.len() >= MAX_ATTRIBUTES {
            return None;
        }
        let equals = rest.find('=')?;
        let key = rest[..equals].trim_end();
        if !is_name(key) {
            return None;
        }
        let value = rest[equals + 1..].trim_start();
        let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let close = value[1..].find(quote)? + 1;
        attributes.push((key.to_owned(), unescape(&value[1..close])?));
        rest = value[close + 1..].trim_start();
    }
    Some((name.to_owned(), attributes))
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || matches!(first, '_' | ':'))
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.'))
}

/// Decodes the predefined entities and character references; `None` for one it does not know.
pub fn unescape(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let end = rest[at..].find(';')? + at;
        let entity = &rest[at + 1..end];
        let decoded = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "apos" => '\'',
            "quot" => '"',
            _ => {
                let number = entity.strip_prefix('#')?;
                let code = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => number.parse().ok()?,
                };
                char::from_u32(code)?
            }
        };
        out.push(decoded);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{Event, MAX_DEPTH, XmlError, read, unescape};

    #[test]
    fn elements_attributes_and_text_are_read_in_order() {
        let events = read(
            b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<!DOCTYPE x [<!ENTITY a \"b\">]><!-- note -->\
              <a k='1 &amp; 2' j=\"&#233;&#x41;\"><b/>text &lt;here&gt;<![CDATA[<raw>]]></a>",
        )
        .unwrap();
        assert_eq!(events.len(), 5, "{events:?}");
        assert_eq!(events[0].attribute("k"), Some("1 & 2"));
        assert_eq!(events[0].attribute("j"), Some("éA"));
        assert!(matches!(&events[1], Event::Start { name, empty: true, .. } if name == "b"));
        assert_eq!(events[2], Event::Text("text <here>".to_owned()));
        assert_eq!(events[3], Event::Text("<raw>".to_owned()));
        assert_eq!(
            events[4],
            Event::End {
                name: "a".to_owned()
            }
        );
    }

    #[test]
    fn broken_documents_are_refused() {
        for broken in [
            &b"<a><b></a>"[..],
            b"<a>",
            b"<a k=1/>",
            b"<a k='1/>",
            b"<a>&bogus;</a>",
            b"<!-- open",
            b"<1a/>",
        ] {
            assert!(
                matches!(read(broken), Err(XmlError::Damaged(_))),
                "{}",
                String::from_utf8_lossy(broken)
            );
        }
        assert_eq!(read(b"<a>\xFF</a>"), Err(XmlError::NotUtf8));
        let deep = "<a>".repeat(MAX_DEPTH + 1);
        assert_eq!(read(deep.as_bytes()), Err(XmlError::TooDeep));
        assert_eq!(unescape("&#xD800;"), None, "a surrogate is no character");
    }
}
