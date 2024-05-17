use crate::parser::{Token};

pub struct SliceRead<'a> {
    slice: &'a [u8],
    index: usize,
}

impl<'a> SliceRead<'a> {
    pub fn new(slice: &'a [u8]) -> Self {
        SliceRead { slice, index: 0 }
    }
    #[inline]
    pub fn next(&mut self) -> Option<u8> {
        if self.index < self.slice.len() {
            let result = self.slice[self.index];
            self.index += 1;
            Some(result)
        } else {
            None
        }
    }
    #[inline]
    pub fn peek(&self) -> Option<u8> {
        if self.index < self.slice.len() {
            Some(self.slice[self.index])
        } else {
            None
        }
    }
    #[inline]
    pub fn skip_whitespace(&mut self) {
        while let Some(&b) = self.slice.get(self.index) {
            if b.is_ascii_whitespace() {
                self.index += 1;
            } else {
                break;
            }
        }
    }
    #[inline]
    pub fn slice_from(&self, start: usize) -> &'a [u8] {
        &self.slice[start..self.index]
    }
    #[inline]
    pub fn is_at_end(&self) -> bool {
        self.index >= self.slice.len()
    }

    #[inline]
    pub fn match_pattern(&mut self, pattern: &[u8]) -> bool {
        let end = self.index + pattern.len();
        if end <= self.slice.len() && self.slice[self.index..end] == *pattern {
            self.index += pattern.len();
            true
        } else {
            false
        }
    }
}


pub struct Lexer<'a> {
    reader: SliceRead<'a>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Lexer {
            reader: SliceRead::new(input),
        }
    }

    pub fn consume_string_until_end_of_array(&mut self) -> Option<&'a str> {
        let mut square_close_count = 1;
        let start = self.reader.index - 1;
        self.reader.skip_whitespace();
        while !self.reader.is_at_end() {
            match self.reader.next()? {
                b'[' => square_close_count += 1,
                b']' => {
                    if square_close_count == 1 {
                        let value = simdutf8::basic::from_utf8(&self.reader.slice[start..self.reader.index - 1]).ok()?;
                        return Some(value);
                    } else {
                        square_close_count -= 1;
                    }
                },
                _ => {}
            }
        }
        None
    }

    pub fn reader_index(&self) -> usize {
        self.reader.index
    }

    pub fn set_reader_index(&mut self, index: usize) {
        self.reader.index = index;
    }

    pub fn consume_string_until_end_of_object(&mut self) -> Option<&'a str> {
        let mut square_close_count = 1;
        let start = self.reader.index - 1;
        self.reader.skip_whitespace();
        while !self.reader.is_at_end() {
            match self.reader.next()? {
                b'{' => square_close_count += 1,
                b'}' => {
                    if square_close_count == 1 {
                        let value = simdutf8::basic::from_utf8(&self.reader.slice[start..self.reader.index]).ok()?;
                        return Some(value);
                    } else {
                        square_close_count -= 1;
                    }
                },
                _ => {}
            }
        }
        None
    }
    #[inline]
    pub fn next_token(&mut self) -> Option<Token<'a>> {
        self.reader.skip_whitespace();

        match self.reader.next()? {
            b'{' => Some(Token::CurlyOpen),
            b'}' => Some(Token::CurlyClose),
            b'[' => Some(Token::SquareOpen),
            b']' => Some(Token::SquareClose),
            b',' => Some(Token::Comma),
            b':' => Some(Token::Colon),
            b'-' | b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' | b'8' | b'9' => {
                let start = self.reader.index - 1;
                while let Some(b) = self.reader.next() {
                    if !((b >= 0x30 && b <= 0x39) || b == b'.') {
                        break;
                    }
                }
                self.reader.index -= 1;
                let s = simdutf8::basic::from_utf8(&self.reader.slice[start..self.reader.index]).ok()?;
                Some(Token::Number(s))
            }
            b'"' => {
                let start = self.reader.index;
                while let Some(b) = self.reader.next() {
                    if b == b'"' && self.reader.slice[self.reader.index - 2] != b'\\' {
                        break; // End of string unless escaped
                    }
                }
                let s = simdutf8::basic::from_utf8(&self.reader.slice[start..self.reader.index - 1]).ok()?;
                Some(Token::String(s))
            }
            b't' if self.reader.match_pattern(b"rue") => Some(Token::Boolean(true)),
            b'f' if self.reader.match_pattern(b"alse") => Some(Token::Boolean(false)),
            b'n' if self.reader.match_pattern(b"ull") => Some(Token::Null),
            // Handle numbers, errors, etc.
            _ => None,
        }
    }
}
