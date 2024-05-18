use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Index;
use crate::concat_string;
use crate::parser::lexer::{Lexer};
use crate::parser::{FlatJsonValue, ParseOptions, ParseResult, PointerFragment, PointerKey, Token, ValueType};

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<Token<'a>>,
    state_seen_start_parse_at: bool,
    pub max_depth: usize,
    root_value_type: ValueType,
    root_array_len: usize,
    pub(crate) depth_after_start_at: usize,
}


impl<'a> Parser<'a> {
    pub fn new(lexer: Lexer<'a>) -> Self {
        Self { lexer, current_token: None, state_seen_start_parse_at: false, max_depth: 0, root_value_type: ValueType::None, root_array_len: 0, depth_after_start_at: 1 }
    }

    pub fn parse(&mut self, parse_option: &ParseOptions, depth: u8) -> Result<ParseResult, String> {
        let mut values: Vec<(PointerKey, Option<String>)> = Vec::with_capacity(10000);
        self.next_token();
        if let Some(current_token) = self.current_token.as_ref() {
            if matches!(current_token, Token::CurlyOpen) {
                let mut pointer_fragment: Vec<String> = Vec::with_capacity(128);
                if let Some(ref p) = parse_option.prefix { pointer_fragment.push(p.clone()) }
                let i = 0;
                self.root_value_type = ValueType::Object;
                self.process(&mut pointer_fragment, &mut values, depth, i, parse_option)?;
                return Ok(ParseResult {
                    json: values,
                    max_json_depth: self.max_depth,
                    parsing_max_depth: parse_option.max_depth,
                    root_value_type: self.root_value_type,
                    root_array_len: self.root_array_len,
                    started_parsing_at: parse_option.start_parse_at.clone(),
                    parsing_prefix: parse_option.prefix.clone(),
                });
            }
            if matches!(current_token, Token::SquareOpen) {
                let mut pointer_fragment: Vec<String> = Vec::with_capacity(128);
                if let Some(ref p) = parse_option.prefix { pointer_fragment.push(p.clone()) }
                let i = 0;
                self.root_value_type = ValueType::Array;
                self.parse_value(&mut pointer_fragment, &mut values, depth, i, parse_option, false)?;
                return Ok(ParseResult {
                    json: values,
                    max_json_depth: self.max_depth,
                    parsing_max_depth: parse_option.max_depth,
                    root_value_type: self.root_value_type,
                    root_array_len: self.root_array_len,
                    started_parsing_at: parse_option.start_parse_at.clone(),
                    parsing_prefix: parse_option.prefix.clone(),
                });
            }
            Err(format!("Expected json to start with {{ or [ but started with {:?}", current_token))
        } else {
            Err("Json is empty".to_string())
        }
    }

    fn process(&mut self, route: &mut PointerFragment, target: &mut FlatJsonValue, depth: u8, count: usize, parse_option: &ParseOptions) -> Result<(), String> {
        if self.max_depth < depth as usize {
            self.max_depth = depth as usize;
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
            if let Some(ref _token) = self.current_token {
                match self.current_token {
                    Some(ref token) if matches!(token, Token::Colon) => {
                        self.next_token();
                    }
                    _ => return Err("Expected ':' after object key".to_string()),
                }
            } else {
                return Err("Expected ':' after object key".to_string());
            }
            self.parse_value(route, target, depth, count, parse_option, true)?;
            self.next_token();


            match self.current_token {
                Some(ref token) if matches!(token, Token::Comma) => {
                    self.next_token();
                }
                Some(ref token) if matches!(token, Token::CurlyClose) => {
                    route.pop();
                    break;
                }
                Some(ref token) if matches!(token, Token::SquareClose) => {
                    route.pop();
                    break;
                }
                None => break,
                _ => return Err(format!("Expected ',' or '}}' or ']' after object value, got: {:?}", self.current_token)),
            }
            route.pop();
        }
        Ok(())
    }

    fn parse_value(&mut self, route: &mut PointerFragment, target: &mut FlatJsonValue, depth: u8, count: usize, parse_option: &ParseOptions, from_object: bool) -> Result<(), String> {
        match self.current_token {
            Some(ref token) => match token {
                Token::CurlyOpen => {
                    if depth <= parse_option.max_depth as u8 {
                        let start = self.lexer.reader_index();
                        if let Some(object_str) = self.lexer.consume_string_until_end_of_object() {
                            target.push((PointerKey::from_pointer(Self::concat_route(route), ValueType::Object, depth), Some(object_str.to_string())));
                            self.lexer.set_reader_index(start);
                            self.process(route, target, depth + 1, count, parse_option);
                            Ok(())
                        } else {
                            Ok(())
                        }
                    } else  {
                        panic!("Should not be there {}",Self::concat_route(route) );
                        target.push((PointerKey::from_pointer(Self::concat_route(route), ValueType::Object, depth), None));
                        self.process(route, target, depth + 1, count, parse_option);
                        Ok(())
                    }
                }
                Token::SquareOpen => {
                    self.next_token();
                    while let Some(ref token) = self.current_token {
                        if matches!(token, Token::SquareClose) {
                            route.pop();
                            break;
                        }
                        if parse_option.parse_array || (parse_option.start_parse_at.is_some() && !self.state_seen_start_parse_at && parse_option.start_parse_at.as_ref().unwrap().eq(&Self::concat_route(route))) {
                            route.push("/0".to_string());
                            self.parse_value(route, target, depth, count, parse_option, false);
                            route.pop();
                            self.next_token();
                            let mut i = 1;
                            while let Some(ref token) = self.current_token {
                                if !matches!(token, Token::Comma) {
                                    if !self.state_seen_start_parse_at {
                                        self.state_seen_start_parse_at = true;
                                        self.root_value_type = ValueType::Array;
                                        self.root_array_len = i;
                                        self.depth_after_start_at = (depth + 1) as usize;
                                    }
                                    break;
                                }
                                self.next_token();
                                if let Some(ref _token) = self.current_token {
                                    route.push(format!("/{}", i));
                                    self.parse_value(route, target, depth, count, parse_option, false);
                                    route.pop();
                                } else {
                                    break;
                                }
                                self.next_token();
                                i += 1;
                            }
                        } else if let Some(array_str) = self.lexer.consume_string_until_end_of_array() {
                            if depth <= parse_option.max_depth as u8 {
                                target.push((PointerKey::from_pointer(Self::concat_route(route), ValueType::Array, depth), Some(concat_string!("[", array_str, "]"))));
                            }
                            break;
                        }
                    }
                    Ok(())
                }
                Token::String(value) => {
                    let value = value.to_string();
                    if depth <= parse_option.max_depth as u8 {
                        let pointer = Self::concat_route(route);
                        if let Some(ref start_parse_at) = parse_option.start_parse_at {
                            if pointer.starts_with(start_parse_at) {
                                target.push((PointerKey::from_pointer(pointer, ValueType::String, depth), Some(value)));
                            }
                        } else {
                            target.push((PointerKey::from_pointer(pointer, ValueType::String, depth), Some(value)));
                        }
                    }

                    Ok(())
                }
                Token::Number(value) => {
                    let value = value.to_string();
                    if depth <= parse_option.max_depth as u8 {
                        let pointer = Self::concat_route(route);
                        if let Some(ref start_parse_at) = parse_option.start_parse_at {
                            if pointer.starts_with(start_parse_at) {
                                target.push((PointerKey::from_pointer(pointer, ValueType::Number, depth), Some(value)));
                            }
                        } else {
                            target.push((PointerKey::from_pointer(pointer, ValueType::Number, depth), Some(value)));
                        }
                    }
                    Ok(())
                }
                Token::Boolean(value) => {
                    let value = *value;
                    if depth <= parse_option.max_depth as u8 {
                        let pointer = Self::concat_route(route);
                        if let Some(ref start_parse_at) = parse_option.start_parse_at {
                            if pointer.starts_with(start_parse_at) {
                                target.push((PointerKey::from_pointer(pointer, ValueType::Bool, depth), Some(value.to_string())));
                            }
                        } else {
                            target.push((PointerKey::from_pointer(pointer, ValueType::Bool, depth), Some(value.to_string())));
                        }
                    }
                    Ok(())
                }
                Token::Null => {
                    if depth <= parse_option.max_depth as u8 {
                        let pointer = Self::concat_route(route);
                        if let Some(ref start_parse_at) = parse_option.start_parse_at {
                            if pointer.starts_with(start_parse_at) {
                                target.push((PointerKey::from_pointer(pointer, ValueType::Null, depth), None));
                            }
                        } else {
                            target.push((PointerKey::from_pointer(pointer, ValueType::Null, depth), None));
                        }
                    }
                    Ok(())
                }
                _ => Err(format!("Unexpected token: {:?}", token))
            },
            _ => Err("Unexpected end of input".to_string())
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
    use crate::parser::{JSONParser, ParseOptions};
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
        let result1 = parser.parse(ParseOptions::default().max_depth(1)).unwrap();
        let vec = &result1.json;
        println!("{:?}", vec);
        assert_eq!(vec.len(), 1);
        assert_eq!(vec[0].0.pointer, "/nested");
        assert_eq!(vec[0].0.value_type, ValueType::Object);
        assert_eq!(vec[0].1, Some("{\"a1\": \"a\",\"b\": {\"a2\": \"a\",\"c\": {\"a3\": \"a\"}}}".to_string()));
        let mut parser = JSONParser::new(json);
        let result2 = parser.parse(ParseOptions::default().max_depth(2)).unwrap();
        let vec = &result2.json;
        println!("{:?}", vec);
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0].0.pointer, "/nested");
        assert_eq!(vec[0].0.value_type, ValueType::Object);
        assert_eq!(vec[0].1, Some("{\"a1\": \"a\",\"b\": {\"a2\": \"a\",\"c\": {\"a3\": \"a\"}}}".to_string()));
        assert_eq!(vec[1].0.pointer, "/nested/a1");
        assert_eq!(vec[1].0.value_type, ValueType::String);
        assert_eq!(vec[1].1, Some("a".to_string()));
        assert_eq!(vec[2].0.pointer, "/nested/b");
        assert_eq!(vec[2].0.value_type, ValueType::Object);
        assert_eq!(vec[2].1, Some("{\"a2\": \"a\",\"c\": {\"a3\": \"a\"}}".to_string()));
        let result3 = JSONParser::change_depth(result1, ParseOptions::default().max_depth(2)).unwrap();
        let vec = &result3.json;
        println!("{:?}", vec);
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0].0.pointer, "/nested");
        assert_eq!(vec[0].0.value_type, ValueType::Object);
        assert_eq!(vec[0].1, Some("{\"a1\": \"a\",\"b\": {\"a2\": \"a\",\"c\": {\"a3\": \"a\"}}}".to_string()));
        assert_eq!(vec[1].0.pointer, "/nested/a1");
        assert_eq!(vec[1].0.value_type, ValueType::String);
        assert_eq!(vec[1].1, Some("a".to_string()));
        assert_eq!(vec[2].0.pointer, "/nested/b");
        assert_eq!(vec[2].0.value_type, ValueType::Object);
        assert_eq!(vec[2].1, Some("{\"a2\": \"a\",\"c\": {\"a3\": \"a\"}}".to_string()));
    }

    #[test]
    fn max_depth_object2() {
        let json = r#"{"skills": [{"description": "Bash", "bonusToTarget": [{"level":1,"value":2}], "copyflags": {
        "plagiarism": true,"reproduce": true}, "bonusToSelf": [{"level":1,"value":2}]}, {"description": "Bash", "copyflags": {"plagiarism": true,"reproduce": true}}]"#;

        let mut parser = JSONParser::new(json);
        let result1 = parser.parse(ParseOptions::default().parse_array(false).start_parse_at("/skills".to_string()).max_depth(1)).unwrap();
        let vec = &result1.json;
        println!("{:?}", vec);
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
        assert_eq!(vec[4].0.pointer, "/flags");
        assert_eq!(vec[4].0.value_type, ValueType::Object);
        assert_eq!(vec[5].0.pointer, "/flags/a");
        assert_eq!(vec[5].0.value_type, ValueType::Bool);
        assert_eq!(vec[5].1, Some("true".to_string()));
        assert_eq!(vec[6].0.pointer, "/flags/b");
        assert_eq!(vec[6].0.value_type, ValueType::Bool);
        assert_eq!(vec[6].1, Some("false".to_string()));
        assert_eq!(vec[7].0.pointer, "/flags/c");
        assert_eq!(vec[7].0.value_type, ValueType::Object);
        assert_eq!(vec[8].0.pointer, "/flags/c/nested");
        assert_eq!(vec[8].0.value_type, ValueType::String);
        assert_eq!(vec[8].1, Some("Oui".to_string()));
    }

    #[test]
    fn simple_array() {
        let json = r#"
            [1,2,3]
        "#;

        let mut parser = JSONParser::new(json);
        let res = parser.parse(ParseOptions::default()).unwrap();
        let vec = res.json;
        println!("{:?}", vec);
        assert_eq!(res.root_array_len, 3);
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
        assert_eq!(vec[1].0.pointer, "/skills/0/description");
        assert_eq!(vec[1].0.parent(), "/skills/0");
        assert_eq!(vec[1].0.value_type, ValueType::String);
        assert_eq!(vec[1].1, Some("Basic Skill".to_string()));
        assert_eq!(vec[3].0.pointer, "/skills/1/description");
        assert_eq!(vec[3].0.value_type, ValueType::String);
        assert_eq!(vec[3].1, Some("Heal".to_string()));
        assert_eq!(vec[5].0.pointer, "/skills/2/description");
        assert_eq!(vec[5].0.value_type, ValueType::String);
        assert_eq!(vec[5].1, Some("Bash".to_string()));
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
        let vec = parser.parse(ParseOptions::default().start_parse_at("/skills".to_string()).parse_array(false)).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec.len(), 9);
        assert_eq!(vec[1].0.pointer, "/skills/0/description");
        assert_eq!(vec[1].0.value_type, ValueType::String);
        assert_eq!(vec[2].0.pointer, "/skills/0/inner");
        assert_eq!(vec[2].0.value_type, ValueType::Array);
        assert_eq!(vec[4].0.pointer, "/skills/1/description");
        assert_eq!(vec[4].0.value_type, ValueType::String);
        assert_eq!(vec[5].0.pointer, "/skills/1/inner");
        assert_eq!(vec[5].0.value_type, ValueType::Array);
        assert_eq!(vec[7].0.pointer, "/skills/2/description");
        assert_eq!(vec[7].0.value_type, ValueType::String);
        assert_eq!(vec[8].0.pointer, "/skills/2/inner");
        assert_eq!(vec[8].0.value_type, ValueType::Array);
    }

    #[test]
    fn array_with_parse_option_false() {
        let json = r#"
            {
                "skills": [
                    {"description": "Basic Skill"},
                    {"description": "Heal"},
                    {"description": "Bash"}
                ]
            }
        "#;

        let mut parser = JSONParser::new(json);
        let vec = parser.parse(ParseOptions::default().parse_array(false)).unwrap().json;
        println!("{:?}", vec);
        assert_eq!(vec[0].0.pointer, "/skills");
        assert_eq!(vec[0].0.value_type, ValueType::Array);
        assert_eq!(vec[0].1.as_ref().unwrap(), "[{\"description\": \"Basic Skill\"},\n                    {\"description\": \"Heal\"},\n                    {\"description\": \"Bash\"}\n                ]");
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
      "copyflags": {
        "plagiarism": true,
        "reproduce": true
      },
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
        let res = parser.parse(ParseOptions::default().parse_array(false).start_parse_at("/skills".to_string()).max_depth(1)).unwrap();
        let vec = res.json;
        println!("{:?}", res.root_array_len);
        println!("{:?}", vec);
    }
}