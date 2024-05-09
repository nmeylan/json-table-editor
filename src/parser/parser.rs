use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Index;
use crate::flatten::Column;
use crate::parser::my_lexer::{Lexer};
use crate::parser::{ParseOptions, Token};

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<Token<'a>>,
    state_seen_start_parse_at: bool,
    pub max_depth: usize,
    pub unique_fields: Vec<Column>,
}

#[derive(Debug)]
pub struct PointerKey {
    pub pointer: String,
    pub value_type: ValueType,
}

impl PartialEq<Self> for PointerKey {
    fn eq(&self, other: &Self) -> bool {
        self.pointer.eq(&other.pointer)
    }
}

impl Eq for PointerKey {}

impl Hash for PointerKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pointer.hash(state);
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

impl PointerKey {
    pub fn from_pointer(pointer: String, value_type: ValueType) -> Self {
        Self {
            pointer,
            value_type
        }
    }
}

#[derive(Eq, Hash, PartialEq, Debug)]
pub enum ValueType {
    Array,
    Object,
    Number,
    String,
    Bool,
    Null,
    None,
}

type PointerFragment = Vec<String>;
pub type FlatJsonValue = Vec<(PointerKey, Option<String>)>;

pub struct ParseResult {
    pub json: FlatJsonValue,
    pub max_json_depth: usize,
    pub parsing_max_depth: usize,
}

impl<'a> Parser<'a> {
    pub fn new(lexer: Lexer<'a>) -> Self {
        Self { lexer, current_token: None, state_seen_start_parse_at: false, max_depth: 0, unique_fields: Vec::with_capacity(1000) }
    }

    pub fn parse(&mut self, parse_option: ParseOptions) -> Result<ParseResult, String> {
        let mut values: Vec<(PointerKey, Option<String>)> = Vec::with_capacity(1_000_000);
        self.next_token();
        if let Some(current_token) = self.current_token.as_ref() {
            if matches!(current_token, Token::CurlyOpen) {
                let mut pointer_fragment: Vec<String> = Vec::with_capacity(128);
                let mut i = 0;
                self.process(&mut pointer_fragment, &mut values, 1, i, &parse_option)?;
                return Ok(ParseResult {
                    json: values,
                    max_json_depth: self.max_depth,
                    parsing_max_depth: parse_option.max_depth,
                })
            }
            if  matches!(current_token, Token::SquareOpen) {
                let mut pointer_fragment: Vec<String> = Vec::with_capacity(128);
                let mut i = 0;
                self.parse_value(&mut pointer_fragment, &mut values, 1, i, &parse_option)?;
                return Ok(ParseResult {
                    json: values,
                    max_json_depth: self.max_depth,
                    parsing_max_depth: parse_option.max_depth,
                })
            }
            return Err(format!("Expected json to start with {{ or [ but started with {:?}", current_token));
        } else {
            return Err("Json is empty".to_string());
        }

    }

    fn process(&mut self, route: &mut PointerFragment, target: &mut FlatJsonValue, depth: usize, count: usize, parse_option: &ParseOptions) -> Result<(), String> {
        if self.max_depth < depth {
            self.max_depth = depth;
        }
        self.next_token();
        while let Some(ref token) = self.current_token {
            match token {
                Token::String(key) => {
                    route.push(concat_string!("/", key));
                }
                _ => return Err("Expected object to have a key at this location".to_string())
            }
            self.next_token();
            if let Some(ref token) = self.current_token {
                match self.current_token {
                    Some(ref token) if matches!(token, Token::Colon) => {
                        self.next_token();
                    }
                    _ => return Err("Expected ':' after object key".to_string()),
                }
            } else {
                return Err("Expected ':' after object key".to_string());
            }
            self.parse_value(route, target, depth, count, parse_option)?;
            self.next_token();


            match self.current_token {
                Some(ref token) if matches!(token, Token::Comma) => {
                    self.next_token();
                }
                Some(ref token) if matches!(token, Token::CurlyClose) => {
                    route.pop();
                    break
                },
                Some(ref token) if matches!(token, Token::SquareClose) => {
                    route.pop();
                    break
                },
                None => break,
                _ => return Err(format!("Expected ',' or '}}' or ']' after object value, got: {:?}", self.current_token)),
            }
            route.pop();
        }
        Ok(())
    }

    fn parse_value(&mut self, route: &mut PointerFragment, target: &mut FlatJsonValue, depth: usize, count: usize, parse_option: &ParseOptions) -> Result<(), String> {
        match self.current_token {
            Some(ref token) => match token {
                Token::CurlyOpen => {
                    if depth < parse_option.max_depth {
                        self.process(route, target, depth + 1, count, parse_option)
                    } else {
                        if let Some(object_str) = self.lexer.consume_string_until_end_of_object() {
                            target.push((PointerKey::from_pointer(Self::concat_route(route), ValueType::Object), Some(object_str.to_string())));
                            Ok(())
                        } else {
                            Ok(())
                        }
                    }
                }
                Token::SquareOpen => {
                    self.next_token();
                    while let Some(ref token) = self.current_token {
                        if matches!(token, Token::SquareClose) {
                            route.pop();
                            break;
                        }
                        if parse_option.parse_array || (parse_option.start_parse_at.is_some() && !self.state_seen_start_parse_at && parse_option.start_parse_at.as_ref().unwrap().eq(&Self::concat_route(route))){
                            route.push("/0".to_string());
                            self.parse_value(route, target, depth, count, parse_option);
                            route.pop();
                            self.next_token();
                            let mut i = 1;
                            while let Some(ref token) = self.current_token {
                                if !matches!(token, Token::Comma) {
                                    if !parse_option.parse_array {
                                        self.state_seen_start_parse_at = true;
                                    }
                                    break;
                                }
                                self.next_token();
                                if let Some(ref token) = self.current_token {
                                    route.push(format!("/{}", i));
                                    self.parse_value(route, target, depth, count, parse_option);
                                    route.pop();
                                } else {
                                    break;
                                }
                                self.next_token();
                                i += 1;
                            }
                        } else {
                            if let Some(array_str) = self.lexer.consume_string_until_end_of_array() {
                                target.push((PointerKey::from_pointer(Self::concat_route(route), ValueType::Array), Some(array_str.to_string())));
                                break;
                            }
                        }

                    }
                    Ok(())
                }
                Token::String(value) => {
                    let value = value.to_string();
                    let pointer = Self::concat_route(route);
                    if let Some(ref start_parse_at) = parse_option.start_parse_at {
                        if pointer.starts_with(start_parse_at) {
                            target.push((PointerKey::from_pointer(pointer, ValueType::String), Some(value)));
                        }
                    } else {
                        target.push((PointerKey::from_pointer(pointer, ValueType::String), Some(value)));
                    }
                    Ok(())
                }
                Token::Number(value) => {
                    let value = value.to_string();
                    let pointer = Self::concat_route(route);
                    if let Some(ref start_parse_at) = parse_option.start_parse_at {
                        if pointer.starts_with(start_parse_at) {
                            target.push((PointerKey::from_pointer(pointer, ValueType::Number), Some(value)));
                        }
                    } else {
                        target.push((PointerKey::from_pointer(pointer, ValueType::Number), Some(value)));
                    }
                    Ok(())
                }
                Token::Boolean(value) => {
                    let value = *value;
                    let pointer = Self::concat_route(route);
                    if let Some(ref start_parse_at) = parse_option.start_parse_at {
                        if pointer.starts_with(start_parse_at) {
                            target.push((PointerKey::from_pointer(pointer, ValueType::Bool), Some(value.to_string())));
                        }
                    } else {
                        target.push((PointerKey::from_pointer(pointer, ValueType::Bool), Some(value.to_string())));
                    }
                    Ok(())
                }
                _ => return Err(format!("Unexpected token: {:?}", token))
            },
            _ => return Err("Unexpected end of input".to_string())
        }
    }
    #[inline]
    fn concat_route(route: &PointerFragment) -> String {
        let i = mem::size_of_val(route);
        let mut res = String::with_capacity(i);
        for p in route {
            res.push_str(p.as_str());
        }
        res
    }
    #[inline]
    fn next_token(&mut self) {
        self.current_token = self.lexer.next_token();
    }
}


#[cfg(test)]
mod tests {
    use crate::parser::{JSONParser, my_lexer, ParseOptions};
    use crate::parser::parser::ValueType;


    #[test]
    fn object() {
        let json = r#"
        {
              "id": 1,
              "maxLevel": 99,
              "name": "NV_BASIC",
              "aaa": true
            }"#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default()).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec[0].0.pointer, "/id");
        assert_eq!(vec[0].0.value_type, ValueType::Number);
        assert_eq!(vec[0].1, Some("1".to_string()));
        assert_eq!(vec[1].0.pointer, "/maxLevel");
        assert_eq!(vec[1].0.value_type, ValueType::Number);
        assert_eq!(vec[1].1, Some("99".to_string()));
        assert_eq!(vec[2].0.pointer, "/name");
        assert_eq!(vec[2].0.value_type, ValueType::String);
        assert_eq!(vec[2].1, Some("NV_BASIC".to_string()));
        assert_eq!(vec[3].0.pointer, "/aaa");
        assert_eq!(vec[3].0.value_type, ValueType::Bool);
        assert_eq!(vec[3].1, Some("true".to_string()));
    }
    #[test]
    fn max_depth_object() {
        let json = r#"{"nested": {"a1": "a","b": {"a2": "a","c": {"a3": "a"}}}"#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default().max_depth(1)).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec.len(), 1);
        assert_eq!(vec[0].0.pointer, "/nested");
        assert_eq!(vec[0].0.value_type, ValueType::Object);
        assert_eq!(vec[0].1, Some("{\"a1\": \"a\",\"b\": {\"a2\": \"a\",\"c\": {\"a3\": \"a\"}}".to_string()));
        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default().max_depth(2)).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec.len(), 2);
        assert_eq!(vec[0].0.pointer, "/nested/a1");
        assert_eq!(vec[0].0.value_type, ValueType::String);
        assert_eq!(vec[0].1, Some("a".to_string()));
        assert_eq!(vec[1].0.pointer, "/nested/b");
        assert_eq!(vec[1].0.value_type, ValueType::Object);
        assert_eq!(vec[1].1, Some("{\"a2\": \"a\",\"c\": {\"a3\": \"a\"}".to_string()));

    }

    #[test]
    fn nested_object() {
        let json = r#"
        {
              "id": 1,
              "maxLevel": 99,
              "name": "NV_BASIC",
              "aaa": true,
              "flags": {"a": true, "b": false, "c": {"nested": "Oui"}}
            }"#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default()).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec[0].0.pointer, "/id");
        assert_eq!(vec[0].0.value_type, ValueType::Number);
        assert_eq!(vec[0].1, Some("1".to_string()));
        assert_eq!(vec[1].0.pointer, "/maxLevel");
        assert_eq!(vec[1].0.value_type, ValueType::Number);
        assert_eq!(vec[1].1, Some("99".to_string()));
        assert_eq!(vec[2].0.pointer, "/name");
        assert_eq!(vec[2].0.value_type, ValueType::String);
        assert_eq!(vec[2].1, Some("NV_BASIC".to_string()));
        assert_eq!(vec[3].0.pointer, "/aaa");
        assert_eq!(vec[3].0.value_type, ValueType::Bool);
        assert_eq!(vec[3].1, Some("true".to_string()));
        assert_eq!(vec[4].0.pointer, "/flags/a");
        assert_eq!(vec[4].0.value_type, ValueType::Bool);
        assert_eq!(vec[4].1, Some("true".to_string()));
        assert_eq!(vec[5].0.pointer, "/flags/b");
        assert_eq!(vec[5].0.value_type, ValueType::Bool);
        assert_eq!(vec[5].1, Some("false".to_string()));
        assert_eq!(vec[6].0.pointer, "/flags/c/nested");
        assert_eq!(vec[6].0.value_type, ValueType::String);
        assert_eq!(vec[6].1, Some("Oui".to_string()));
    }

    #[test]
    fn simple_array() {
        let json = r#"
            [1,2,3]
        "#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default()).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec[0].0.pointer, "/0");
        assert_eq!(vec[0].0.value_type, ValueType::Number);
        assert_eq!(vec[0].1, Some("1".to_string()));
        assert_eq!(vec[1].0.pointer, "/1");
        assert_eq!(vec[1].0.value_type, ValueType::Number);
        assert_eq!(vec[1].1, Some("2".to_string()));
        assert_eq!(vec[2].0.pointer, "/2");
        assert_eq!(vec[2].0.value_type, ValueType::Number);
        assert_eq!(vec[2].1, Some("3".to_string()));
    }

    #[test]
    fn simple_array_nested() {
        let json = r#"
            [[1],[2],[3]]
        "#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default()).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec[0].0.pointer, "/0/0");
        assert_eq!(vec[0].0.value_type, ValueType::Number);
        assert_eq!(vec[0].1, Some("1".to_string()));
        assert_eq!(vec[1].0.pointer, "/1/0");
        assert_eq!(vec[1].0.value_type, ValueType::Number);
        assert_eq!(vec[1].1, Some("2".to_string()));
        assert_eq!(vec[2].0.pointer, "/2/0");
        assert_eq!(vec[2].0.value_type, ValueType::Number);
        assert_eq!(vec[2].1, Some("3".to_string()));
    }
    #[test]
    fn array() {
        let json = r#"
            {
                "skills": [
                    {"description": "Basic Skill"},
                    {"description": "Heal"},
                    {"description": "Bash"},
                ]
            }
        "#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default()).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec[0].0.pointer, "/skills/0/description");
        assert_eq!(vec[0].0.value_type, ValueType::String);
        assert_eq!(vec[0].1, Some("Basic Skill".to_string()));
        assert_eq!(vec[1].0.pointer, "/skills/1/description");
        assert_eq!(vec[1].0.value_type, ValueType::String);
        assert_eq!(vec[1].1, Some("Heal".to_string()));
        assert_eq!(vec[2].0.pointer, "/skills/2/description");
        assert_eq!(vec[2].0.value_type, ValueType::String);
        assert_eq!(vec[2].1, Some("Bash".to_string()));
    }

    #[test]
    fn array_with_start_parse_at() {
        let json = r#"
            {
                "skills": [
                    {"description": "Basic Skill", "inner": [2]},
                    {"description": "Heal", "inner": [3]},
                    {"description": "Bash", "inner": [1]}
                ]
            }
        "#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default().start_parse_at("/skills").parse_array(false)).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec.len(), 6);
        assert_eq!(vec[0].0.pointer, "/skills/0/description");
        assert_eq!(vec[0].0.value_type, ValueType::String);
        assert_eq!(vec[1].0.pointer, "/skills/0/inner");
        assert_eq!(vec[1].0.value_type, ValueType::Array);
        assert_eq!(vec[2].0.pointer, "/skills/1/description");
        assert_eq!(vec[2].0.value_type, ValueType::String);
        assert_eq!(vec[3].0.pointer, "/skills/1/inner");
        assert_eq!(vec[3].0.value_type, ValueType::Array);
        assert_eq!(vec[4].0.pointer, "/skills/2/description");
        assert_eq!(vec[4].0.value_type, ValueType::String);
        assert_eq!(vec[5].0.pointer, "/skills/2/inner");
        assert_eq!(vec[5].0.value_type, ValueType::Array);
    }

    #[test]
    fn array_with_parse_option_false() {
        let json = r#"
            {
                "skills": [
                    {"description": "Basic Skill"},
                    {"description": "Heal"},
                    {"description": "Bash"},
                ]
            }
        "#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default().parse_array(false)).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec[0].0.pointer, "/skills");
        assert_eq!(vec[0].0.value_type, ValueType::Array);
        assert_eq!(parser.parser.unique_fields[0].name, "/description");
    }


    #[test]
    fn complex_array() {
        let json = r#"
        {
          "skills": [
            {
      "description": "Basic Skill",
      "id": 1,
      "maxLevel": 9,
      "name": "NV_BASIC",
      "basicSkillPerLevel": [
        {
          "level": 1,
          "value": "Trade"
        },
        {
          "level": 2,
          "value": "Emoticon"
        },
        {
          "level": 3,
          "value": "Sit"
        },
        {
          "level": 4,
          "value": "Chat Room (create)"
        },
        {
          "level": 5,
          "value": "Party (join)"
        },
        {
          "level": 6,
          "value": "Kafra Storage"
        },
        {
          "level": 7,
          "value": "Party (create)"
        },
        {
          "level": 8,
          "value": "-"
        },
        {
          "level": 9,
          "value": "Job Change"
        }
      ],
      "targetType": "Passive"
    },
    {
      "description": "Sword Mastery",
      "id": 2,
      "maxLevel": 10,
      "name": "SM_SWORD",
      "type": "Weapon",
      "bonusToSelf": [
        {
          "level": 1,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 4
          }
        },
        {
          "level": 2,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 8
          }
        },
        {
          "level": 3,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 12
          }
        },
        {
          "level": 4,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 16
          }
        },
        {
          "level": 5,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 20
          }
        },
        {
          "level": 6,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 24
          }
        },
        {
          "level": 7,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 28
          }
        },
        {
          "level": 8,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 32
          }
        },
        {
          "level": 9,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 36
          }
        },
        {
          "level": 10,
          "value": {
            "bonus": "MasteryDamageUsingWeaponType",
            "value": "1hSword",
            "value2": 40
          }
        }
      ],
      "targetType": "Passive"
    }
          ]
        }"#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default().parse_array(false).start_parse_at("/skills")).unwrap().json;
        println!("{:?}", vec);
    }
}