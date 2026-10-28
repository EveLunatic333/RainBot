use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use lazy_static::lazy_static;

#[derive(Debug, Clone)]
pub struct PendingEdit {
    pub user_id: u64,
    pub trigger: String,
    pub saved_image_filename: Option<String>,
    pub new_diameter: Option<i64>,
    pub new_x_start: Option<i64>,
    pub new_y_start: Option<i64>,
    pub new_animated: Option<bool>,
    pub new_bg_width: Option<i64>,
    pub new_bg_height: Option<i64>,
    pub new_role_id: Option<u64>,
    pub created_at: u64,
}

lazy_static! {
    pub static ref PENDING_EDITS: Mutex<HashMap<String, PendingEdit>> = Mutex::new(HashMap::new());
}

pub fn make_key(user_id: u64, trigger: &str) -> String {
    format!("{}_{}", user_id, trigger)
}

pub async fn save(key: String, data: PendingEdit) {
    let mut cache = PENDING_EDITS.lock().await;
    cache.insert(key, data);
}

pub async fn take(key: &str) -> Option<PendingEdit> {
    let mut cache = PENDING_EDITS.lock().await;
    cache.remove(key)
}

pub fn new_pending_edit(
    user_id: u64,
    trigger: String,
    saved_image_filename: Option<String>,
    new_diameter: Option<i64>,
    new_x_start: Option<i64>,
    new_y_start: Option<i64>,
    new_animated: Option<bool>,
    new_bg_width: Option<i64>,
    new_bg_height: Option<i64>,
    new_role_id: Option<u64>,
) -> PendingEdit {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    PendingEdit {
        user_id,
        trigger,
        saved_image_filename,
        new_diameter,
        new_x_start,
        new_y_start,
        new_animated,
        new_bg_width,
        new_bg_height,
        new_role_id,
        created_at,
    }
}