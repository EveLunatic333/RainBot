use regex::Regex;
use std::collections::HashMap;

pub struct BountyRegexGenerator;

impl BountyRegexGenerator {

    pub fn bbq_trigger() -> String {
        r"!bbq(0[1-9]|1[0-9]|2[0-5])".to_string()
    }

    pub fn category_trigger() -> String {
        r"!(bronze|silver|gold|free|event)\s*bounty".to_string()
    }

    pub fn check_submission() -> String {
        r"!check\s+(?:<@!?(\d+)>|(\w+))".to_string()
    }

    pub fn reward_info() -> String {
        r"!reward\s+(?:bronze|silver|gold|free|event|bbq(?:0[1-9]|1[0-9]|2[0-5]))?".to_string()
    }

    pub fn cooldown_check() -> String {
        r"!(?:cd|cooldown)".to_string()
    }

    pub fn title_info() -> String {
        r"!title\s+(?:bronze|silver|gold)\s+(?:bounty|trading)".to_string()
    }

    pub fn gacha_pull() -> String {
        r"!(?:gacha\s+)?pull".to_string()
    }

    pub fn market_search() -> String {
        r"!market\s+(.+)".to_string()
    }

    pub fn compile_all() -> HashMap<&'static str, Regex> {
        let mut patterns = HashMap::new();
        
        patterns.insert("bbq", Regex::new(&Self::bbq_trigger()).unwrap());
        patterns.insert("category", Regex::new(&Self::category_trigger()).unwrap());
        patterns.insert("check", Regex::new(&Self::check_submission()).unwrap());
        patterns.insert("reward", Regex::new(&Self::reward_info()).unwrap());
        patterns.insert("cooldown", Regex::new(&Self::cooldown_check()).unwrap());
        patterns.insert("title", Regex::new(&Self::title_info()).unwrap());
        patterns.insert("gacha", Regex::new(&Self::gacha_pull()).unwrap());
        patterns.insert("market", Regex::new(&Self::market_search()).unwrap());
        
        patterns
    }

    pub fn extract_bbq_number(text: &str) -> Option<u8> {
        let re = Regex::new(r"!bbq(0[1-9]|1[0-9]|2[0-5])").unwrap();
        re.captures(text)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<u8>().ok())
    }

    pub fn extract_category(text: &str) -> Option<String> {
        let re = Regex::new(r"!(bronze|silver|gold|free|event)\s*bounty").unwrap();
        re.captures(text)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
    }

    pub fn extract_user_id(text: &str) -> Option<u64> {
        let re = Regex::new(r"!check\s+(?:<@!?(\d+)>|(\w+))").unwrap();
        re.captures(text)
            .and_then(|cap| {
                cap.get(1)
                    .or_else(|| cap.get(2))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bbq_regex() {
        let re = Regex::new(&BountyRegexGenerator::bbq_trigger()).unwrap();
        assert!(re.is_match("!bbq01"));
        assert!(re.is_match("!bbq15"));
        assert!(re.is_match("!bbq25"));
        assert!(!re.is_match("!bbq00"));
        assert!(!re.is_match("!bbq26"));
    }

    #[test]
    fn test_category_regex() {
        let re = Regex::new(&BountyRegexGenerator::category_trigger()).unwrap();
        assert!(re.is_match("!bronze bounty"));
        assert!(re.is_match("!gold bounty"));
        assert!(!re.is_match("!diamond bounty"));
    }

    #[test]
    fn test_extract_bbq() {
        assert_eq!(BountyRegexGenerator::extract_bbq_number("!bbq15"), Some(15));
        assert_eq!(BountyRegexGenerator::extract_bbq_number("!bbq01"), Some(1));
        assert_eq!(BountyRegexGenerator::extract_bbq_number("!bbq99"), None);
    }
}