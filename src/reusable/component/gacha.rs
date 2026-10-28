use crate::reusable::image_edit::gacha::{GachaData, GachaR};
use crate::reusable::postgress::gacha::GachaPg;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use super::super::bitwise::ItemCode;
use super::MyErr;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GachaConfig {
    pub cost_single: i32,
    pub cost_multi: i32,
    pub pity_threshold: i32,
    pub rate_ur: f32,
    pub rate_ssr1: f32,
    pub rate_ssr2: f32,
    pub rate_sr1: f32,
    pub rate_sr2: f32,
    pub rate_sr3: f32,
    pub rate_r1: f32,
    pub rate_r2: f32,
    pub guaranteed_ur: f32,
    pub guaranteed_ssr1: f32,
}

impl Default for GachaConfig {
    fn default() -> Self {
        GachaConfig {
            cost_single: 10,
            cost_multi: 110,
            pity_threshold: 50,
            rate_ur: 0.001,
            rate_ssr1: 0.009,
            rate_ssr2: 0.030,
            rate_sr1: 0.060,
            rate_sr2: 0.150,
            rate_sr3: 0.250,
            rate_r1: 0.300,
            rate_r2: 0.200,
            guaranteed_ur: 0.15,
            guaranteed_ssr1: 0.85,
        }
    }
}

impl GachaConfig {
    pub fn validate(&self) -> Result<(), String> {
        let total = self.rate_ur + self.rate_ssr1 + self.rate_ssr2
            + self.rate_sr1 + self.rate_sr2 + self.rate_sr3
            + self.rate_r1 + self.rate_r2;
        if (total - 1.0).abs() > 0.0001 {
            return Err(format!(
                "Total Gacha rate must be exactly 1.0 (100%), but detected: {:.4}. Please fix your gacha.json!",
                total
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Gacha {
    pub config: GachaConfig,
    ur: Vec<ItemCode>,
    ssr1: Vec<ItemCode>,
    ssr2: Vec<ItemCode>,
    sr1: Vec<ItemCode>,
    sr2: Vec<ItemCode>,
    sr3: Vec<ItemCode>,
    r1: Vec<ItemCode>,
    r2: Vec<ItemCode>,
}

impl Default for Gacha {
    fn default() -> Self {
        Gacha {
            config: GachaConfig::default(),
            ur: Vec::new(), ssr1: Vec::new(), ssr2: Vec::new(),
            sr1: Vec::new(), sr2: Vec::new(), sr3: Vec::new(),
            r1: Vec::new(), r2: Vec::new(),
        }
    }
}

impl Gacha {
    pub async fn new() -> Result<Gacha, MyErr> {
        let path = Path::new(".").join("static").join("gacha.json");
        let data = tokio::fs::read_to_string(&path).await?;
        let decoded: Self = serde_json::from_str(&data)?;
        decoded.config.validate().map_err(MyErr::Custom)?;
        Ok(decoded)
    }

    pub async fn check(data: &[u8]) -> Result<(), MyErr> {
        let decoded: Self = serde_json::from_slice(data)?;
        decoded.config.validate().map_err(MyErr::Custom)?;
        let path = Path::new(".").join("static").join("gacha.json");
        tokio::fs::write(path, data.to_vec()).await?;
        Ok(())
    }

    fn emergency_fallback(&self) -> (GachaData, bool) {
        let mut thread = rand::thread_rng();
        if !self.r2.is_empty() { return (GachaData { code: self.r2.choose(&mut thread).unwrap().to_owned(), result: GachaR::R }, false); }
        if !self.r1.is_empty() { return (GachaData { code: self.r1.choose(&mut thread).unwrap().to_owned(), result: GachaR::R }, false); }
        if !self.sr3.is_empty() { return (GachaData { code: self.sr3.choose(&mut thread).unwrap().to_owned(), result: GachaR::SR }, false); }
        if !self.sr2.is_empty() { return (GachaData { code: self.sr2.choose(&mut thread).unwrap().to_owned(), result: GachaR::SR }, false); }
        if !self.sr1.is_empty() { return (GachaData { code: self.sr1.choose(&mut thread).unwrap().to_owned(), result: GachaR::SR }, false); }
        if !self.ssr2.is_empty() { return (GachaData { code: self.ssr2.choose(&mut thread).unwrap().to_owned(), result: GachaR::SSR }, false); }
        if !self.ssr1.is_empty() { return (GachaData { code: self.ssr1.choose(&mut thread).unwrap().to_owned(), result: GachaR::SSR }, true); }
        if !self.ur.is_empty() { return (GachaData { code: self.ur.choose(&mut thread).unwrap().to_owned(), result: GachaR::UR }, true); }
        panic!("FATAL ERROR: ALL GACHA ARRAYS ARE EMPTY IN JSON!");
    }

    fn pull(&self) -> (GachaData, bool) {
        let mut thread = rand::thread_rng();
        let rng: f32 = thread.gen();
        let cfg = &self.config;
        let mut cumulative = 0.0;

        cumulative += cfg.rate_ur;
        if rng <= cumulative && !self.ur.is_empty() {
            return (GachaData { code: self.ur.choose(&mut thread).unwrap().to_owned(), result: GachaR::UR }, true);
        }

        cumulative += cfg.rate_ssr1;
        if rng <= cumulative && !self.ssr1.is_empty() {
            return (GachaData { code: self.ssr1.choose(&mut thread).unwrap().to_owned(), result: GachaR::SSR }, true);
        }

        cumulative += cfg.rate_ssr2;
        if rng <= cumulative && !self.ssr2.is_empty() {
            return (GachaData { code: self.ssr2.choose(&mut thread).unwrap().to_owned(), result: GachaR::SSR }, false);
        }

        cumulative += cfg.rate_sr1;
        if rng <= cumulative && !self.sr1.is_empty() {
            return (GachaData { code: self.sr1.choose(&mut thread).unwrap().to_owned(), result: GachaR::SR }, false);
        }

        cumulative += cfg.rate_sr2;
        if rng <= cumulative && !self.sr2.is_empty() {
            return (GachaData { code: self.sr2.choose(&mut thread).unwrap().to_owned(), result: GachaR::SR }, false);
        }

        cumulative += cfg.rate_sr3;
        if rng <= cumulative && !self.sr3.is_empty() {
            return (GachaData { code: self.sr3.choose(&mut thread).unwrap().to_owned(), result: GachaR::SR }, false);
        }

        cumulative += cfg.rate_r1;
        if rng <= cumulative && !self.r1.is_empty() {
            return (GachaData { code: self.r1.choose(&mut thread).unwrap().to_owned(), result: GachaR::R }, false);
        }

        cumulative += cfg.rate_r2;
        if rng <= cumulative && !self.r2.is_empty() {
            return (GachaData { code: self.r2.choose(&mut thread).unwrap().to_owned(), result: GachaR::R }, false);
        }

        self.emergency_fallback()
    }

    fn guaranteed(&self) -> GachaData {
        let mut thread = rand::thread_rng();
        let rng: f32 = thread.gen();
        let cfg = &self.config;

        if rng <= cfg.guaranteed_ur && !self.ur.is_empty() {
            return GachaData { code: self.ur.choose(&mut thread).unwrap().to_owned(), result: GachaR::UR };
        }

        if !self.ssr1.is_empty() {
            return GachaData { code: self.ssr1.choose(&mut thread).unwrap().to_owned(), result: GachaR::SSR };
        }

        if !self.ur.is_empty() {
            return GachaData { code: self.ur.choose(&mut thread).unwrap().to_owned(), result: GachaR::UR };
        }

        self.emergency_fallback().0
    }

    pub fn single_pull(&self, data: &GachaPg) -> (GachaPg, GachaData) {
        let mut pity = data.pity + 1;
        let ticket = data.ticket - self.config.cost_single;

        if pity >= self.config.pity_threshold {
            return (GachaPg { pity: 0, ticket }, self.guaranteed());
        }

        let (pull_data, reset_pity) = self.pull();
        if reset_pity { pity = 0; }

        (GachaPg { pity, ticket }, pull_data)
    }

    pub fn multi_pull(&self, data: &GachaPg) -> (GachaPg, Vec<GachaData>) {
        let mut pity = data.pity;
        let ticket = data.ticket - self.config.cost_multi;
        let mut res = Vec::new();

        for _ in 0..12 {
            pity += 1;
            if pity >= self.config.pity_threshold {
                res.push(self.guaranteed());
                pity = 0;
            } else {
                let (pull_data, reset_pity) = self.pull();
                if reset_pity { pity = 0; }
                res.push(pull_data);
            }
        }
        (GachaPg { pity, ticket }, res)
    }
}