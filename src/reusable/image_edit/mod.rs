pub mod gacha;
pub mod huntercard;
pub mod title;

use gacha::Gacha;

#[derive(Debug, Clone)]
pub struct Images {
    pub gacha: Gacha,
}

impl Images {
    pub async fn new() -> Result<Images, CustomImageError> {
        let gacha = Gacha::new().await?;
        Ok(Images { gacha })
    }
}

#[derive(Debug)]
pub enum CustomImageError {
    Custom(&'static str),
    Tokio(tokio::io::Error),
    Image(image::ImageError),
    Reqwest(reqwest::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for CustomImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CustomImageError::Custom(x) => x.fmt(f),
            CustomImageError::Tokio(x) => x.fmt(f),
            CustomImageError::Image(x) => x.fmt(f),
            CustomImageError::Reqwest(x) => x.fmt(f),
            CustomImageError::Json(x) => x.fmt(f),
        }
    }
}

impl std::error::Error for CustomImageError {}

impl From<tokio::io::Error> for CustomImageError {
    fn from(value: tokio::io::Error) -> Self {
        CustomImageError::Tokio(value)
    }
}

impl From<image::ImageError> for CustomImageError {
    fn from(value: image::ImageError) -> Self {
        CustomImageError::Image(value)
    }
}

impl From<reqwest::Error> for CustomImageError {
    fn from(value: reqwest::Error) -> Self {
        CustomImageError::Reqwest(value)
    }
}

impl From<serde_json::Error> for CustomImageError {
    fn from(value: serde_json::Error) -> Self {
        CustomImageError::Json(value)
    }
}