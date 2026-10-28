use serde::Deserialize;
use reqwest::Client;
use std::sync::LazyLock;
use std::time::Duration;

#[derive(Deserialize, Debug)]
pub struct SightResponse {
    pub status: String,
    pub nudity: Option<NudityScore>,
    pub gore: Option<GoreScore>,
    pub offensive: Option<OffensiveScore>,
}

#[derive(Deserialize, Debug)]
pub struct NudityScore {
    pub sexual_activity: f64,
    pub sexual_display: f64,
    pub erotica: f64,
}

#[derive(Deserialize, Debug)]
pub struct GoreScore {
    pub prob: f64,
}

#[derive(Deserialize, Debug)]
pub struct OffensiveScore {
    pub prob: f64,
}

pub struct SightengineKeys {
    pub api_user: String,
    pub api_secret: String,
}

/// Sightengine credentials are read from the environment only.
///
/// Both `SIGHTENGINE_API_USER` and `SIGHTENGINE_API_SECRET` must be set and
/// non-empty. When either is missing the moderation feature degrades to
/// "disabled" (a single warning is logged) instead of panicking, so the bot
/// still boots on a fresh clone that has not configured moderation yet.
pub static SIGHTENGINE_KEYS: LazyLock<Option<SightengineKeys>> = LazyLock::new(|| {
    let api_user = std::env::var("SIGHTENGINE_API_USER").unwrap_or_default();
    let api_secret = std::env::var("SIGHTENGINE_API_SECRET").unwrap_or_default();

    if api_user.trim().is_empty() || api_secret.trim().is_empty() {
        tracing::warn!(
            "SIGHTENGINE_API_USER / SIGHTENGINE_API_SECRET are not set; image moderation is disabled"
        );
        return None;
    }

    Some(SightengineKeys { api_user, api_secret })
});

pub async fn scan_image(image_url: &str, api_user: &str, api_secret: &str) -> bool {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    let url = "https://api.sightengine.com/1.0/check.json";
    let params = [
        ("url", image_url),
        ("models", "nudity-2.0,gore,offensive"),
        ("api_user", api_user),
        ("api_secret", api_secret),
    ];

    let response = match client.get(url).query(&params).send().await {
        Ok(res) => res,
        Err(_) => return false,
    };

    let data: SightResponse = match response.json().await {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };

    if data.status == "success" {
        if let Some(n) = data.nudity {
            if n.sexual_activity > 0.45 || n.sexual_display > 0.5 || n.erotica > 0.6 {
                return true;
            }
        }
        if let Some(g) = data.gore {
            if g.prob > 0.5 {
                return true;
            }
        }
        if let Some(o) = data.offensive {
            if o.prob > 0.65 {
                return true;
            }
        }
    }

    false
}