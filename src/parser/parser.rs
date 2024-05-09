use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Index;
use crate::parser::my_lexer::{Lexer};
use crate::parser::{ParseOptions, Token};

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<Token<'a>>,
}

#[derive(Debug)]
pub struct PointerKey {
    pub pointer: String,
    pub value_type: ValueType,
    pub index: usize,
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
    pub fn from_pointer(pointer: String, value_type: ValueType, index: usize) -> Self {
        Self {
            pointer,
            value_type,
            index,
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

impl<'a> Parser<'a> {
    pub fn new(lexer: Lexer<'a>) -> Self {
        Self { lexer, current_token: None }
    }

    pub fn parse(&mut self, parse_array: ParseOptions) -> Result<FlatJsonValue, String> {
        let mut values: Vec<(PointerKey, Option<String>)> = Vec::with_capacity(1_000_000);
        self.next_token();
        if let Some(current_token) = self.current_token.as_ref() {
            if matches!(current_token, Token::CurlyOpen) {
                let mut pointer_fragment: Vec<String> = Vec::with_capacity(128);
                let mut i = 0;
                self.process(&mut pointer_fragment, &mut values, 0, i, parse_array.parse_array)?;
                return Ok(values)
            }
            if  matches!(current_token, Token::SquareOpen) {
                let mut pointer_fragment: Vec<String> = Vec::with_capacity(128);
                let mut i = 0;
                self.parse_value(&mut pointer_fragment, &mut values, 0, i, parse_array.parse_array)?;
                return Ok(values)
            }
            return Err(format!("Expected json to start with {{ or [ but started with {:?}", current_token));
        } else {
            return Err("Json is empty".to_string());
        }

    }

    fn process(&mut self, route: &mut PointerFragment, target: &mut FlatJsonValue, depth: i32, count: usize, parse_array: bool) -> Result<(), String> {
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
            self.parse_value(route, target, depth, count, parse_array)?;
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

    fn parse_value(&mut self, route: &mut PointerFragment, target: &mut FlatJsonValue, depth: i32, count: usize, parse_array: bool) -> Result<(), String> {
        match self.current_token {
            Some(ref token) => match token {
                Token::CurlyOpen => { self.process(route, target, depth, count, parse_array) }
                Token::SquareOpen => {
                    self.next_token();
                    while let Some(ref token) = self.current_token {
                        if matches!(token, Token::SquareClose) {
                            route.pop();
                            break;
                        }
                        if parse_array {
                            route.push("/0".to_string());
                            self.parse_value(route, target, depth, count, parse_array);
                            route.pop();
                            self.next_token();
                            let mut i = 1;
                            while let Some(ref token) = self.current_token {
                                if !matches!(token, Token::Comma) {
                                    break;
                                }
                                self.next_token();
                                if let Some(ref token) = self.current_token {
                                    route.push(format!("/{}", i));
                                    self.parse_value(route, target, depth, count, parse_array);
                                    route.pop();
                                } else {
                                    break;
                                }
                                self.next_token();
                                i += 1;
                            }
                        } else {
                            if let Some(array_str) = self.lexer.consume_string_until() {
                                target.push((PointerKey::from_pointer(Self::concat_route(route), ValueType::Array, count), Some(array_str.to_string())));
                                break;
                            }
                        }

                    }
                    Ok(())
                }
                Token::String(value) => {
                    let value = value.to_string();
                    // self.next_token();
                    target.push((PointerKey::from_pointer(Self::concat_route(route), ValueType::String, count), Some(value)));
                    Ok(())
                }
                Token::Number(value) => {
                    let value = value.to_string();
                    // self.next_token();
                    target.push((PointerKey::from_pointer(Self::concat_route(route), ValueType::Number, count), Some(value)));
                    Ok(())
                }
                Token::Boolean(value) => {
                    let value = *value;
                    // self.next_token();
                    target.push((PointerKey::from_pointer(Self::concat_route(route), ValueType::Bool, count), Some(value.to_string())));
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
        let vec = parser.parse(ParseOptions::default()).unwrap();
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
        let vec = parser.parse(ParseOptions::default()).unwrap();
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
        let vec = parser.parse(ParseOptions::default()).unwrap();
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
        let vec = parser.parse(ParseOptions::default()).unwrap();
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
        let vec = parser.parse(ParseOptions::default()).unwrap();
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
        let vec = parser.parse(ParseOptions::default().parse_array(false)).unwrap();
        println!("{:?}", vec);
        assert_eq!(vec[0].0.pointer, "/skills");
        assert_eq!(vec[0].0.value_type, ValueType::Array);
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
        let vec = parser.parse(ParseOptions::default().parse_array(false)).unwrap();
        println!("{:?}", vec);
    }
}