use sled::IVec;
use uuid::Uuid;
use serde::Serialize;
use std::fmt::Debug;

use crate::db::config::get_db;

pub fn save_webhook_to_db<T: Serialize>(webhook: &T) where T: Serialize + Debug, {
    let db = get_db();
    // Generate unique ID
    let uuid = Uuid::new_v4();
    let id = uuid.as_bytes();
    // Serialize webhook struct to JSON
    let parse_res = serde_json::to_vec(webhook);
    if parse_res.is_err() {
        let e = parse_res.expect_err("No error in parse_res in save_webhook_to_db");
        log::error!("[save_webhook_to_db] Failed to serialize webhook: {:?}", e);
        return;
    }
    let webhook_json = parse_res.expect("Uncaught error in parse_res webhook");
    // Insert JSON into sled DB
    let insert_res = db.insert(IVec::from(id), webhook_json);
    if insert_res.is_err() {
        let e = insert_res.expect_err("No error in insert_res");
        log::error!("[save_webhook_to_db] Failed to upsert webhook into sled DB: {e}");
        return;
    }
    log::debug!("[save_webhook_to_db] Webhook succesfully upserted: {:?}", webhook);
}