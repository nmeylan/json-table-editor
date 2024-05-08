use crate::parser::my_lexer::Lexer;
use crate::parser::parser::{FlatJsonValue, Parser};

pub mod parser;
pub mod my_lexer;

pub struct JSONParser<'a> {
    pub parser: Parser<'a>,
}

pub struct ParseOptions {
    pub parse_array: bool,
    pub depth: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            parse_array: true,
            depth: 10,
        }
    }
}
impl ParseOptions {
    fn parse_array(mut self, parse_array: bool) -> Self {
        self.parse_array = parse_array;
        self
    }
}

impl<'a> JSONParser<'a> {
    pub fn new(input: &'a str) -> Self {
        let lexer = Lexer::new(input.as_bytes());
        let parser = Parser::new(lexer);

        Self { parser }
    }
    pub fn parse(&mut self, options: ParseOptions) -> Result<FlatJsonValue, String> {
        self.parser.parse(options)
    }
}


#[derive(Debug)]
pub enum Token<'a> {
    CurlyOpen,
    CurlyClose,
    SquareOpen,
    SquareClose,
    Colon,
    Comma,
    String(&'a str),
    Number(&'a str),
    Boolean(bool),
    Null,
}