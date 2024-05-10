use egui::ahash::{HashSet, HashSetExt};
use crate::parser::lexer::Lexer;
use crate::parser::parser::{FlatJsonValue, Parser, ParseResult, PointerKey, ValueType};
use crate::table::Column;

pub mod parser;
pub mod lexer;

pub struct JSONParser<'a> {
    pub parser: Parser<'a>,
}

#[derive(Clone)]
pub struct ParseOptions {
    pub parse_array: bool,
    pub max_depth: usize,
    pub start_parse_at: Option<String>,
    pub prefix: Option<String>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            parse_array: true,
            max_depth: 10,
            start_parse_at: None,
            prefix: None,
        }
    }
}

impl ParseOptions {
    pub fn parse_array(mut self, parse_array: bool) -> Self {
        self.parse_array = parse_array;
        self
    }

    pub fn start_parse_at(mut self, pointer: String) -> Self {
        self.start_parse_at = Some(pointer);
        self
    }
    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }
    pub fn prefix(mut self, prefix: String) -> Self {
        self.prefix = Some(prefix);
        self
    }
}

#[derive(Debug, Clone)]
pub struct JsonArrayEntries {
    entries: FlatJsonValue,
    index: usize,
}

impl JsonArrayEntries {
    pub fn entries(&self) -> &FlatJsonValue {
        &self.entries
    }
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn find_node_at(&self, pointer: &str) -> Option<&(PointerKey, Option<String>)> {
        self.entries().iter().find(|(p, _)| p.pointer.eq(pointer))
    }

}

#[macro_export]
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
        self.parser.parse(&options, 1)
    }

    pub fn change_depth_array(previous_parse_result: ParseResult, mut json_array: Vec<JsonArrayEntries>, depth: usize) -> Result<(Vec<JsonArrayEntries>, Vec<Column>), String> {
        let len = json_array.len();
        let mut new_json_array = Vec::with_capacity(json_array.len());
        let mut unique_keys: Vec<Column> = Vec::with_capacity(1000);
        for i in (0..len).rev() {
            let mut parse_result = previous_parse_result.clone_except_json();
            parse_result.json = json_array.pop().unwrap().entries;
            let mut options = ParseOptions::default().parse_array(false).max_depth(depth);
            let result = Self::change_depth(parse_result, options)?;
            let mut vec = result.json;

            for j in 0..vec.len() {
                let (k, _v) = &mut vec[j];
                let _i = i.to_string();
                let (prefix_len) = if let Some(ref started_parsing_at) = previous_parse_result.started_parsing_at {
                    let prefix = concat_string!(started_parsing_at, "/", _i);
                    prefix.len()
                } else if let Some(ref prefix) = previous_parse_result.parsing_prefix {
                    let prefix = concat_string!(prefix, "/", _i);
                    prefix.len()
                } else {
                    let prefix = concat_string!("/", _i);
                    prefix.len()
                };
                if !k.pointer.is_empty() {
                    if k.pointer.len() <= prefix_len {
                        println!("ERROR, depth {} out of bounds of {}, expected to have a prefix of len {}", depth, k.pointer, prefix_len);
                        continue;
                    }
                    let key = &k.pointer[prefix_len..k.pointer.len()];
                    let column = Column {
                        name: key.to_string(),
                        depth: k.depth,
                        value_type: k.value_type,
                    };
                    if !unique_keys.contains(&column) && !column.name.contains("#") {
                        unique_keys.push(column);
                    }
                }
                k.index = i;
            }
            new_json_array.push(JsonArrayEntries { entries: vec, index: i });
        }
        new_json_array.reverse();
        Ok((new_json_array, unique_keys))
    }

    pub fn change_depth(previous_parse_result: ParseResult, mut parse_options: ParseOptions) -> Result<ParseResult, String> {
        if previous_parse_result.parsing_max_depth < parse_options.max_depth {
            let previous_len = previous_parse_result.json.len();
            let mut new_flat_json_structure = FlatJsonValue::with_capacity(previous_len + (parse_options.max_depth - previous_parse_result.parsing_max_depth) * (previous_len / 3));
            for (k, v) in previous_parse_result.json {
                if !matches!(k.value_type, ValueType::Object) || k.depth > parse_options.max_depth as u8 {
                    new_flat_json_structure.push((k, v));
                } else if let Some(mut v) = v {
                    new_flat_json_structure.push((k.clone(), Some(v.clone())));
                    let lexer = Lexer::new(v.as_bytes());
                    let mut parser = Parser::new(lexer);
                    parse_options.prefix =  Some(k.pointer);
                    let res = parser.parse(&parse_options, k.depth + 1)?;
                    new_flat_json_structure.extend(res.json);
                }
            }
            Ok(ParseResult {
                json: new_flat_json_structure,
                max_json_depth: previous_parse_result.max_json_depth,
                parsing_max_depth: parse_options.max_depth,
                root_value_type: previous_parse_result.root_value_type,
                started_parsing_at: previous_parse_result.started_parsing_at,
                parsing_prefix: previous_parse_result.parsing_prefix,
                root_array_len: previous_parse_result.root_array_len,
            })
        } else if previous_parse_result.parsing_max_depth > parse_options.max_depth {
            // serialization
            todo!("");
        } else {
            Ok(previous_parse_result)
        }
    }

    pub fn as_array(mut previous_parse_result: ParseResult) -> Result<(Vec<JsonArrayEntries>, Vec<Column>), String> {
        if !matches!(previous_parse_result.root_value_type, ValueType::Array) {
            return Err("Parsed json root is not an array".to_string());
        }
        let mut unique_keys: HashSet<Column> = HashSet::with_capacity(1000);
        let mut res: Vec<JsonArrayEntries> = Vec::with_capacity(previous_parse_result.root_array_len);
        let mut j = previous_parse_result.json.len() - 1;
        let mut estimated_capacity = 100;
        for i in (0..previous_parse_result.root_array_len).rev() {
            let mut flat_json_values = FlatJsonValue::with_capacity(estimated_capacity);
            let mut is_first_entry = true;
            let _i = i.to_string();
            loop {
                if j >= 0 && !previous_parse_result.json.is_empty() {
                    let (k, _v) = &previous_parse_result.json[j];
                    let (match_prefix, prefix_len) = if let Some(ref started_parsing_at) = previous_parse_result.started_parsing_at {
                        let prefix = concat_string!(started_parsing_at, "/", _i);
                        (k.pointer.starts_with(&prefix), prefix.len())
                    } else if let Some(ref prefix) = previous_parse_result.parsing_prefix {
                        let prefix = concat_string!(prefix, "/", _i);
                        (k.pointer.starts_with(&prefix), prefix.len())
                    } else {
                        let prefix = concat_string!("/", _i);
                        (k.pointer.starts_with(&prefix), prefix.len())
                    };
                    if !k.pointer.is_empty() {
                        let key = &k.pointer[prefix_len..k.pointer.len()];
                        let column = Column {
                            name: key.to_string(),
                            depth: k.depth,
                            value_type: k.value_type
                        };
                        unique_keys.insert(column);
                    }
                    if match_prefix {
                        if is_first_entry {
                            is_first_entry = false;
                            let prefix = &k.pointer[0..prefix_len];
                            flat_json_values.push((PointerKey::from_pointer_and_index(concat_string!(prefix, "/#"), ValueType::Number, k.depth, i), Some(i.to_string())));
                        }
                        let (mut k, v) = previous_parse_result.json.pop().unwrap();
                        k.index = i;
                        flat_json_values.push((k, v));
                    } else {
                        break;
                    }
                    if j == 0 {
                        break;
                    }
                    j -= 1;
                } else {
                    break;
                }
            }
            res.push(JsonArrayEntries { entries: flat_json_values, index: i });

        }
        res.reverse();
        Ok((res, unique_keys.into_iter().collect()))
    }

    pub fn filter_non_null_column(previous_parse_result: &Vec<JsonArrayEntries>, prefix: &str, non_null_columns: &Vec<String>) -> Vec<JsonArrayEntries> {
        let mut res: Vec<JsonArrayEntries> = Vec::with_capacity(previous_parse_result.len());
        for row in previous_parse_result {
            let mut should_add_row = true;
            for pointer in non_null_columns {
                let pointer_to_find = concat_string!(prefix, "/", row.index().to_string(), pointer);
                if let Some((_, value)) = row.find_node_at(&pointer_to_find) {
                    if value.is_none() {
                        should_add_row = false;
                        break;
                    }
                } else {
                    should_add_row = false;
                    break;
                }
            }

            if should_add_row {
                res.push(row.clone());
            }
        }
        res
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