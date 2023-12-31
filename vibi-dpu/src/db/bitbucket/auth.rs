use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use sled::IVec;

use crate::db::config::get_db;
use crate::utils::bitbucket_auth_info::BitbucketAuthInfo;

pub fn save_bitbucket_auth_info_to_db(auth_info: &mut BitbucketAuthInfo) {
    let db = get_db();
    let now = SystemTime::now();
    let since_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");  
    auth_info.set_timestamp(since_epoch.as_secs());
    log::debug!("[save_bitbucket_auth_info_to_db] auth info = {:?}", &auth_info);
    let json = serde_json::to_string(&auth_info).expect("Failed to serialize auth info");
    // Convert JSON string to bytes
    let bytes = json.as_bytes(); 

    // Create IVec from bytes
    let ivec = IVec::from(bytes);

    // Insert into sled DB
    let insert_res = db.insert("bitbucket_auth_info", ivec);
    if insert_res.is_err() {
        let e = insert_res.expect_err("No error in insert_res");
        log::error!("[save_bitbucket_auth_info_to_db] Failed to upsert bitbucket auth info into sled DB: {e}");
        return;
    }
    log::debug!("[save_bitbucket_auth_info_to_db] BitbucketAuthInfo succesfully upserted: {:?}", auth_info);
}

pub fn bitbucket_auth_info() -> Option<BitbucketAuthInfo> {
    let db = get_db();
	let authinfo_key = "bitbucket_auth_info";
	let authinfo_res = db.get(IVec::from(authinfo_key.as_bytes()));
    if authinfo_res.is_err() {
        let e = authinfo_res.expect_err("No error in authinfo_res");
        log::error!("[bitbucket_auth_info] Unable to get bb authinfo from db: {:?}", e);
        return None;
    }
    let authinfo_opt = authinfo_res.expect("Uncaught error in authinfo_res");
    if authinfo_opt.is_none() {
        log::error!("[bitbucket_auth_info] No bitbucket authinfo in db");
        return None;
    }
    let authinfo_ivec = authinfo_opt.expect("Empty authinfo_opt");
    let authinfo_parse = serde_json::from_slice(&authinfo_ivec);
    if authinfo_parse.is_err() {
        let e = authinfo_parse.expect_err("No error in authinfo_parse");
        log::error!("[bitbucket_auth_info] Unable to deserialize bitbucket authinfo_parse: {:?}", e);
        return None;
    }
	let bitbucket_auth_info: BitbucketAuthInfo =  authinfo_parse.expect("Uncaught error in authinfo_parse");
    return Some(bitbucket_auth_info);
}