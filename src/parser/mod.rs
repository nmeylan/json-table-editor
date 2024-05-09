use std::mem;
use crate::flatten::Column;
use crate::parser::my_lexer::Lexer;
use crate::parser::parser::{FlatJsonValue, Parser, ParseResult, ValueType};

pub mod parser;
pub mod my_lexer;

pub struct JSONParser<'a> {
    pub parser: Parser<'a>,
}

#[derive(Clone)]
pub struct ParseOptions {
    pub parse_array: bool,
    pub max_depth: usize,
    pub start_parse_at: Option<String>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            parse_array: true,
            max_depth: 10,
            start_parse_at: None,
        }
    }
}

impl ParseOptions {
    pub fn parse_array(mut self, parse_array: bool) -> Self {
        self.parse_array = parse_array;
        self
    }

    pub fn start_parse_at(mut self, pointer: &str) -> Self {
        self.start_parse_at = Some(pointer.to_string());
        self
    }
    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }
}


macro_rules! concat_string {
    () => { String::with_capacity(0) };
    ($($s:expr),+) => {{
        use std::ops::AddAssign;
        let mut len = 0;
        $(len.add_assign(AsRef::<str>::as_ref(&$s).len());)+
        let mut buf = String::with_capacity(len);
        $(buf.push_str($s.as_ref());)+
        buf
    }};
}


impl<'a> JSONParser<'a> {
    pub fn new(input: &'a str) -> Self {
        let lexer = Lexer::new(input.as_bytes());
        let parser = Parser::new(lexer);

        Self { parser }
    }
    pub fn parse(&mut self, options: ParseOptions) -> Result<ParseResult, String> {
        self.parser.parse(&options, 1, None)
    }

    pub fn change_depth(previous_parse_result: ParseResult, parse_options: ParseOptions) -> Result<ParseResult, String> {
        if previous_parse_result.parsing_max_depth < parse_options.max_depth {
            let previous_len = previous_parse_result.json.len();
            let mut new_flat_json_structure = FlatJsonValue::with_capacity(previous_len + (parse_options.max_depth - previous_parse_result.parsing_max_depth) * (previous_len / 3));
            for (k, v) in previous_parse_result.json {
                if !matches!(k.value_type, ValueType::Object) || k.depth > parse_options.max_depth as u8 {
                    new_flat_json_structure.push((k, v));
                } else {
                    if let Some(v) = v {
                        let lexer = Lexer::new(v.as_bytes());
                        let mut parser = Parser::new(lexer);
                        let res = parser.parse(&parse_options, k.depth + 1, Some(k.pointer))?;
                        new_flat_json_structure.extend(res.json);
                    }
                }
            }
            Ok(ParseResult {
                json: new_flat_json_structure,
                max_json_depth: previous_parse_result.max_json_depth,
                parsing_max_depth: parse_options.max_depth,
                root_value_type: previous_parse_result.root_value_type,
                started_parsing_at: previous_parse_result.started_parsing_at,
                root_array_len: previous_parse_result.root_array_len,
            })
        } else if previous_parse_result.parsing_max_depth > parse_options.max_depth {
            // serialization
            todo!("");
        } else {
            Ok(previous_parse_result)
        }
    }

    pub fn as_array(mut previous_parse_result: ParseResult) -> Result<(Vec<FlatJsonValue>, Vec<Column>), String> {
        if !matches!(previous_parse_result.root_value_type, ValueType::Array) {
            return Err("Parsed json root is not an array".to_string());
        }
        let mut unique_keys: Vec<Column> = Vec::with_capacity(1000);
        let mut res: Vec<FlatJsonValue> = Vec::with_capacity(previous_parse_result.root_array_len);
        let mut j = previous_parse_result.json.len() - 1;
        let mut estimated_capacity = 1;
        for i in (0..previous_parse_result.root_array_len).rev() {
            let mut flat_json_values = FlatJsonValue::with_capacity(estimated_capacity);
            loop {
                if j > 0 && previous_parse_result.json.len() > 0 {
                    let (k, v) = &previous_parse_result.json[j];
                    let _i = i.to_string();
                    let (match_prefix, prefix_len) = if let Some(ref started_parsing_at) = previous_parse_result.started_parsing_at {
                        let prefix = concat_string!(started_parsing_at, "/", _i);
                        (k.pointer.starts_with(&prefix), prefix.len())
                    } else {
                        let prefix = concat_string!("/", _i);
                        (k.pointer.starts_with(&prefix), prefix.len())
                    };
                    if k.pointer.len() > 0 {
                        // println!("{}({}). - {} {}", i, match_prefix, j, k.pointer);
                        let key = &k.pointer[prefix_len..k.pointer.len()];
                        let column = Column {
                            name: key.to_string(),
                            depth: k.depth,
                        };
                        if !unique_keys.contains(&column) {
                            unique_keys.push(column);
                        }
                    }
                    if match_prefix {
                        let (mut k, v) = previous_parse_result.json.pop().unwrap();
                        k.index = i;
                        flat_json_values.push((k, v));
                    } else {
                        break;
                    }
                    j -=1;
                } else {
                    break;
                }
            }
            res.push(flat_json_values);

            if i == 10 {
                estimated_capacity = j / 10;
            }
        }

        Ok((res, unique_keys))
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