use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::ParseIntError;

pub mod distribution;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ItemCode {
    pub name: Option<String>,
    pub key: String,
    pub count: u16,
    pub types: u8,
}

impl Default for ItemCode {
    fn default() -> Self {
        ItemCode {
            name: None,
            key: "0700".to_string(),
            count: 1,
            types: 7,
        }
    }
}

#[derive(Debug)]
pub enum BitwiseError {
    Custom(String),
    OddLength,
    InvalidKey,
    NoItem,
    DuplicateUser,
    InvalidPassword,
    AlreadyBound,
    CharacterNotOwned,
    ParseInt(ParseIntError),
    Sqlx(sqlx::Error),
}

impl fmt::Display for BitwiseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BitwiseError::OddLength => "Input string has an odd number of bytes".fmt(f),
            BitwiseError::InvalidKey => "The key for converting endian is invalid length".fmt(f),
            BitwiseError::NoItem => "No item on the selected data".fmt(f),
            BitwiseError::DuplicateUser => "Registration failed: This username is already registered!".fmt(f),
            BitwiseError::InvalidPassword => "Authentication failed: The password you entered is incorrect.".fmt(f),
            BitwiseError::AlreadyBound => "Binding failed: This account is already linked to another Discord user.".fmt(f),
            BitwiseError::CharacterNotOwned => "Access denied: You don't have permission to access this character.".fmt(f),
            BitwiseError::ParseInt(e) => e.fmt(f),
            BitwiseError::Sqlx(e) => e.fmt(f),
            BitwiseError::Custom(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for BitwiseError {}

impl From<ParseIntError> for BitwiseError {
    fn from(e: ParseIntError) -> Self {
        BitwiseError::ParseInt(e)
    }
}

impl From<sqlx::Error> for BitwiseError {
    fn from(value: sqlx::Error) -> Self {
        BitwiseError::Sqlx(value)
    }
}

pub struct Bitwise<'a> {
    item: &'a [ItemCode],
}

impl<'a> Bitwise<'a> {
    pub fn decode(hex_value: &str) -> Result<Vec<u8>, BitwiseError> {
        if hex_value.len() % 2 != 0 {
            return Err(BitwiseError::OddLength);
        }
        (0..hex_value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex_value[i..i + 2], 16).map_err(BitwiseError::ParseInt))
            .collect()
    }

    pub fn new(item: &'a [ItemCode]) -> Self {
        Bitwise { item }
    }

    pub fn multiple_item(&self) -> Result<Vec<u8>, BitwiseError> {
        if self.item.is_empty() {
            return Err(BitwiseError::NoItem);
        }
        let mut hex_str = format!("{:04X}", self.item.len());
        for code in self.item {
            hex_str.push_str(&format!(
                "{:02X}0000{}0000{:04X}00000000",
                code.types,
                code.reverse_key()?,
                code.count
            ));
        }
        Self::decode(&hex_str)
    }
}