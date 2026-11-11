pub struct NotShape;

pub struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(line: &'a str) -> Cursor<'a> {
        Cursor {
            bytes: line.as_bytes(),
            at: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    pub fn literal(&mut self, text: &str) -> Result<(), NotShape> {
        let end = self.at.saturating_add(text.len());
        if self.bytes.get(self.at..end) == Some(text.as_bytes()) {
            self.at = end;
            Ok(())
        } else {
            Err(NotShape)
        }
    }

    pub fn string(&mut self) -> Result<String, NotShape> {
        self.literal("\"")?;
        let mut out = Vec::new();
        loop {
            let byte = self.peek().ok_or(NotShape)?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(out).map_err(|_| NotShape),
                b'\\' => {
                    let escaped = self.peek().ok_or(NotShape)?;
                    if escaped != b'"' && escaped != b'\\' {
                        return Err(NotShape);
                    }
                    self.at += 1;
                    out.push(escaped);
                }
                0x00..=0x1f => return Err(NotShape),
                _ => out.push(byte),
            }
        }
    }

    pub fn number(&mut self) -> Result<u64, NotShape> {
        let start = self.at;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        let digits = &self.bytes[start..self.at];
        if digits.is_empty() || (digits.len() > 1 && digits[0] == b'0') {
            return Err(NotShape);
        }
        Ok(digits.iter().fold(0u64, |value, digit| {
            value
                .saturating_mul(10)
                .saturating_add(u64::from(digit - b'0'))
        }))
    }

    pub fn list<T>(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Result<T, NotShape>,
    ) -> Result<Vec<T>, NotShape> {
        self.literal("[")?;
        let mut out = Vec::new();
        if self.literal("]").is_ok() {
            return Ok(out);
        }
        loop {
            out.push(item(self)?);
            if self.literal("]").is_ok() {
                return Ok(out);
            }
            self.literal(",")?;
        }
    }

    pub fn object<T>(
        &mut self,
        mut value: impl FnMut(&mut Self) -> Result<T, NotShape>,
    ) -> Result<Vec<(String, T)>, NotShape> {
        self.literal("{")?;
        let mut out = Vec::new();
        if self.literal("}").is_ok() {
            return Ok(out);
        }
        loop {
            let key = self.string()?;
            self.literal(":")?;
            out.push((key, value(self)?));
            if self.literal("}").is_ok() {
                return Ok(out);
            }
            self.literal(",")?;
        }
    }

    pub fn end(&self) -> Result<(), NotShape> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(NotShape)
        }
    }
}
