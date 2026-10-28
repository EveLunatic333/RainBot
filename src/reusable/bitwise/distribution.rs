use super::{BitwiseError, ItemCode};
use crate::material::ItemPedia;

impl ItemCode {
    pub fn reverse_key(&self) -> Result<String, BitwiseError> {
        if self.key.len() != 4 {
            Err(BitwiseError::InvalidKey)
        } else {
            let chars: Vec<char> = self.key.chars().collect();
            Ok(format!("{}{}{}{}", chars[2], chars[3], chars[0], chars[1]))
        }
    }

    pub fn text(&self, item: &ItemPedia) -> Option<String> {
        let name = item.dictionary(self.types, &self.key)?;
        Some(format!("{} x{}", name, self.count))
    }

    pub fn check(&self, item: &ItemPedia) -> bool {
        item.dictionary(self.types, &self.key).is_some()
    }
}