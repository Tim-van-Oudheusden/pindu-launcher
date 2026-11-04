//! Valve KeyValues ("VDF"): the text form of `libraryfolders.vdf` and `appmanifest_*.acf`,
//! and the binary form inside `appinfo.vdf`.

use std::collections::HashMap;
use std::fmt;

/// A KeyValues node. Keys keep their order and are matched case-insensitively, as Steam
/// does.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f32),
    Map(Vec<(String, Value)>),
}

impl Value {
    /// The first child with this key, ignoring case.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Map(entries) => entries
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// Follows a chain of keys.
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        keys.iter().try_fold(self, |value, key| value.get(key))
    }

    /// The string child with this key.
    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    /// An integer, or a string holding one (text VDF stores every number as a string).
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(number) => Some(*number),
            Self::Str(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    /// The children of a map, in file order.
    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Self::Map(entries) => entries,
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

/// Parses text KeyValues into a map of its top-level entries.
pub fn parse_text(text: &str) -> Result<Value, ParseError> {
    let mut lexer = Lexer { text, pos: 0 };
    let entries = lexer.entries(false)?;
    Ok(Value::Map(entries))
}

enum Token<'a> {
    Open,
    Close,
    Text(std::borrow::Cow<'a, str>),
}

struct Lexer<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn error(&self, message: &'static str) -> ParseError {
        ParseError {
            offset: self.pos,
            message,
        }
    }

    /// Reads `key value` pairs until `}` (when nested) or the end of the text.
    fn entries(&mut self, nested: bool) -> Result<Vec<(String, Value)>, ParseError> {
        let mut entries = Vec::new();
        loop {
            let key = match self.token()? {
                Some(Token::Text(key)) => key.into_owned(),
                Some(Token::Close) if nested => return Ok(entries),
                None if !nested => return Ok(entries),
                None => return Err(self.error("unclosed map")),
                Some(_) => return Err(self.error("expected a key")),
            };
            let value = match self.token()? {
                Some(Token::Open) => Value::Map(self.entries(true)?),
                Some(Token::Text(text)) => Value::Str(text.into_owned()),
                _ => return Err(self.error("expected a value")),
            };
            entries.push((key, value));
        }
    }

    fn token(&mut self) -> Result<Option<Token<'a>>, ParseError> {
        loop {
            self.skip_space_and_comments();
            let Some(next) = self.text[self.pos..].chars().next() else {
                return Ok(None);
            };
            match next {
                '{' => {
                    self.pos += 1;
                    return Ok(Some(Token::Open));
                }
                '}' => {
                    self.pos += 1;
                    return Ok(Some(Token::Close));
                }
                '"' => return self.quoted().map(|text| Some(Token::Text(text))),
                // Platform conditionals such as `[$WIN32]` apply to the entry before them.
                '[' => {
                    let end = self.text[self.pos..]
                        .find(']')
                        .ok_or_else(|| self.error("unclosed conditional"))?;
                    self.pos += end + 1;
                }
                _ => {
                    let rest = &self.text[self.pos..];
                    let end = rest
                        .find(|c: char| c.is_whitespace() || matches!(c, '{' | '}' | '"'))
                        .unwrap_or(rest.len());
                    self.pos += end;
                    return Ok(Some(Token::Text(rest[..end].into())));
                }
            }
        }
    }

    fn skip_space_and_comments(&mut self) {
        loop {
            let rest = &self.text[self.pos..];
            let trimmed = rest.trim_start();
            self.pos += rest.len() - trimmed.len();
            if !trimmed.starts_with("//") {
                return;
            }
            self.pos += trimmed.find('\n').unwrap_or(trimmed.len());
        }
    }

    fn quoted(&mut self) -> Result<std::borrow::Cow<'a, str>, ParseError> {
        let start = self.pos + 1;
        let mut owned: Option<String> = None;
        let mut chars = self.text[start..].char_indices();
        while let Some((index, c)) = chars.next() {
            match c {
                '"' => {
                    self.pos = start + index + 1;
                    return Ok(match owned {
                        Some(text) => text.into(),
                        None => self.text[start..start + index].into(),
                    });
                }
                '\\' => {
                    let text =
                        owned.get_or_insert_with(|| self.text[start..start + index].to_owned());
                    match chars.next().map(|(_, escaped)| escaped) {
                        Some('n') => text.push('\n'),
                        Some('t') => text.push('\t'),
                        Some(escaped) => text.push(escaped),
                        None => break,
                    }
                }
                _ => {
                    if let Some(text) = owned.as_mut() {
                        text.push(c);
                    }
                }
            }
        }
        self.pos = self.text.len();
        Err(self.error("unclosed string"))
    }
}

/// Reads the apps of a Steam `appinfo.vdf` (format versions 27 to 29) for which `wanted`
/// returns true. Each value is the app's `appinfo` map (`common`, `extended`, ...).
pub fn read_appinfo(
    bytes: &[u8],
    wanted: impl Fn(u32) -> bool,
) -> Result<HashMap<u32, Value>, ParseError> {
    let mut reader = Reader { bytes, pos: 0 };
    let magic = reader.u32()?;
    let version = match magic {
        0x0756_4427 => 27,
        0x0756_4428 => 28,
        0x0756_4429 => 29,
        _ => return Err(reader.error("unknown appinfo.vdf format")),
    };
    reader.u32()?; // universe
    let strings = if version >= 29 {
        let offset =
            usize::try_from(reader.i64()?).map_err(|_| reader.error("bad string table offset"))?;
        let mut table = Reader { bytes, pos: offset };
        let count = table.u32()?;
        (0..count)
            .map(|_| table.cstring())
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    let mut apps = HashMap::new();
    loop {
        let app_id = reader.u32()?;
        if app_id == 0 {
            return Ok(apps);
        }
        let size = reader.u32()? as usize;
        let end = reader
            .pos
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| reader.error("truncated app entry"))?;
        if wanted(app_id) {
            // info state, last updated, PICS token, SHA-1, change number, binary SHA-1
            let header = 4 + 4 + 8 + 20 + 4 + if version >= 28 { 20 } else { 0 };
            let mut entry = Reader {
                bytes: &bytes[..end],
                pos: reader.pos + header,
            };
            let root = entry.binary_map(&strings)?;
            let info = match root.get("appinfo") {
                Some(info) => info.clone(),
                None => root,
            };
            apps.insert(app_id, info);
        }
        reader.pos = end;
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn error(&self, message: &'static str) -> ParseError {
        ParseError {
            offset: self.pos,
            message,
        }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], ParseError> {
        let bytes = self
            .bytes
            .get(self.pos..self.pos + N)
            .ok_or_else(|| self.error("unexpected end of file"))?;
        self.pos += N;
        Ok(bytes.try_into().expect("slice has N bytes"))
    }

    fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32, ParseError> {
        self.take().map(u32::from_le_bytes)
    }

    fn i64(&mut self) -> Result<i64, ParseError> {
        self.take().map(i64::from_le_bytes)
    }

    fn cstring(&mut self) -> Result<String, ParseError> {
        let rest = self.bytes.get(self.pos..).unwrap_or_default();
        let len = rest
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| self.error("unterminated string"))?;
        let text = String::from_utf8_lossy(&rest[..len]).into_owned();
        self.pos += len + 1;
        Ok(text)
    }

    fn key(&mut self, strings: &[String]) -> Result<String, ParseError> {
        if strings.is_empty() {
            return self.cstring();
        }
        let index = self.u32()? as usize;
        strings
            .get(index)
            .cloned()
            .ok_or_else(|| self.error("key outside the string table"))
    }

    /// Reads typed entries until the end-of-map marker.
    fn binary_map(&mut self, strings: &[String]) -> Result<Value, ParseError> {
        let mut entries = Vec::new();
        loop {
            let kind = self.u8()?;
            if matches!(kind, 0x08 | 0x0B) {
                return Ok(Value::Map(entries));
            }
            let key = self.key(strings)?;
            let value = match kind {
                0x00 => self.binary_map(strings)?,
                0x01 => Value::Str(self.cstring()?),
                // int32, pointer and colour are all four-byte integers
                0x02 | 0x04 | 0x06 => Value::Int(i64::from(self.take().map(i32::from_le_bytes)?)),
                0x03 => Value::Float(self.take().map(f32::from_le_bytes)?),
                0x07 => Value::Int(self.take().map(u64::from_le_bytes)? as i64),
                0x0A => Value::Int(self.i64()?),
                _ => return Err(self.error("unknown value type")),
            };
            entries.push((key, value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_nested_maps_and_strings_parse() {
        let root = parse_text(
            r#"
            "AppState"
            {
                "appid"		"570"
                "name"		"Dota 2"
                "InstalledDepots" { "571" { "size" "42" } }
            }
            "#,
        )
        .unwrap();

        let app = root.get("AppState").unwrap();
        assert_eq!(app.str("appid"), Some("570"));
        assert_eq!(app.str("name"), Some("Dota 2"));
        assert_eq!(
            app.path(&["InstalledDepots", "571", "size"])
                .and_then(Value::as_str),
            Some("42")
        );
    }

    #[test]
    fn text_keys_match_case_insensitively() {
        let root = parse_text(r#""AppState" { "StateFlags" "4" }"#).unwrap();

        assert_eq!(
            root.path(&["appstate", "stateflags"])
                .and_then(Value::as_str),
            Some("4")
        );
    }

    #[test]
    fn text_escapes_comments_and_unquoted_tokens_parse() {
        let root = parse_text(
            "// a comment\n\"a\" { \"path\" \"C:\\\\Games\\\\\\\"x\\\"\" key value [$WIN32] }",
        )
        .unwrap();

        let a = root.get("a").unwrap();
        assert_eq!(a.str("path"), Some(r#"C:\Games\"x""#));
        assert_eq!(a.str("key"), Some("value"));
    }

    #[test]
    fn text_with_an_unclosed_map_is_an_error() {
        assert!(parse_text(r#""a" { "b" "c" "#).is_err());
    }

    #[test]
    fn text_with_a_key_and_no_value_is_an_error() {
        assert!(parse_text(r#""a" { "b" }"#).is_err());
    }

    #[test]
    fn integer_values_read_as_numbers() {
        let map = Value::Map(vec![
            ("n".into(), Value::Int(7)),
            ("s".into(), Value::Str("8".into())),
        ]);

        assert_eq!(map.get("n").and_then(Value::as_i64), Some(7));
        assert_eq!(map.get("s").and_then(Value::as_i64), Some(8));
    }
}
