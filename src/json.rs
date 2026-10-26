//! Just enough JSON to read LRCLIB's answers. Malformed input yields `None`.

pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(text: &str) -> Option<Json> {
        let mut p = Parser {
            s: text.as_bytes(),
            i: 0,
            depth: 0,
        };
        let value = p.value()?;
        p.ws();
        (p.i == p.s.len()).then_some(value)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn is_true(&self) -> bool {
        matches!(self, Json::Bool(true))
    }

    pub fn items(&self) -> &[Json] {
        match self {
            Json::Arr(items) => items,
            _ => &[],
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: u32,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Option<Json> {
        self.s[self.i..].starts_with(word.as_bytes()).then(|| {
            self.i += word.len();
            value
        })
    }

    fn value(&mut self) -> Option<Json> {
        self.ws();
        // Nesting limit: a hostile answer must not overflow the stack.
        self.depth += 1;
        if self.depth > 32 {
            return None;
        }
        let v = match *self.s.get(self.i)? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Json::Str),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            _ => self.number(),
        };
        self.depth -= 1;
        v
    }

    fn object(&mut self) -> Option<Json> {
        self.i += 1;
        let mut fields = Vec::new();
        if self.eat(b'}') {
            return Some(Json::Obj(fields));
        }
        loop {
            self.ws();
            let key = self.string()?;
            if !self.eat(b':') {
                return None;
            }
            fields.push((key, self.value()?));
            if self.eat(b'}') {
                return Some(Json::Obj(fields));
            }
            if !self.eat(b',') {
                return None;
            }
        }
    }

    fn array(&mut self) -> Option<Json> {
        self.i += 1;
        let mut items = Vec::new();
        if self.eat(b']') {
            return Some(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            if self.eat(b']') {
                return Some(Json::Arr(items));
            }
            if !self.eat(b',') {
                return None;
            }
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.i;
        while self
            .s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_digit() || b"+-.eE".contains(c))
        {
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        text.parse().ok().map(Json::Num)
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.s.get(self.i..self.i + 4)?;
        self.i += 4;
        u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        if self.s.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    let ch = match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let mut code = self.hex4()?;
                            // A UTF-16 surrogate pair spans two escapes.
                            if (0xD800..0xDC00).contains(&code)
                                && self.s.get(self.i..self.i + 2) == Some(b"\\u")
                            {
                                self.i += 2;
                                let low = self.hex4()?;
                                code = 0x10000
                                    + ((code - 0xD800) << 10)
                                    + (low.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        other => other as char,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_values() {
        let j =
            Json::parse(r#" {"a": [1, -2.5e1, true, null], "s": "x\"\né🎵", "o": {}} "#).unwrap();
        let a = j.get("a").unwrap().items();
        assert_eq!(a[1].num(), Some(-25.0));
        assert!(a[2].is_true());
        assert_eq!(j.get("s").and_then(Json::str), Some("x\"\né🎵"));
        assert!(Json::parse("{\"a\":1,}").is_none());
        assert!(Json::parse(&"[".repeat(100)).is_none());
    }
}
