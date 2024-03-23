use std::env;
use std::str;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::db::bitbucket::auth::{save_bitbucket_auth_info_to_db, bitbucket_auth_info};
use crate::utils::gitops::set_git_remote_url;
use crate::utils::reqwest_client::get_client;
use crate::utils::bitbucket_auth_info::BitbucketAuthInfo;
use crate::utils::review::Review;

pub async fn get_access_token_from_bitbucket(code: &str) -> Option<BitbucketAuthInfo> {
    let client = get_client();
    let bitbucket_client_id = env::var("BITBUCKET_CLIENT_ID").unwrap();
    let bitbucket_client_secret = env::var("BITBUCKET_CLIENT_SECRET").unwrap();
    let mut params = std::collections::HashMap::new();
    let redirect_uri = format!("{}/api/bitbucket/callbacks/install",
        env::var("SERVER_URL").expect("SERVER_URL must be set"));
    params.insert("client_id", bitbucket_client_id);
    params.insert("client_secret", bitbucket_client_secret);
    params.insert("code", code.to_owned());
    params.insert("grant_type", "authorization_code".to_owned());
    params.insert("redirect_uri", redirect_uri);
    log::debug!("[get_access_token_from_bitbucket] params = {:?}", &params);
    let post_res = client
        .post("https://bitbucket.org/site/oauth2/access_token")
        .form(&params)
        .send()
        .await;
    if post_res.is_err() {
        let e = post_res.expect_err("No error in post_res");
        log::error!("[get_access_token_from_bitbucket] error in calling api : {:?}", e);
        return None;
    }
    let res = post_res.expect("Uncaught error in post_res");
    if !res.status().is_success() {
        log::error!(
            "[get_access_token_from_bitbucket] Failed to exchange code for access token. Status code: {}, Response content: {:?}",
            res.status(),
            res.text().await
        );
        return None;
    }
    let parse_res = res.json::<BitbucketAuthInfo>().await ;
    if parse_res.is_err() {
        let e = parse_res.expect_err("No error in parse_res for BitbucketAuthInfo");
        log::error!("[get_access_token_from_bitbucket] error deserializing BitbucketAuthInfo: {:?}", e);
        return None;
    }
    let mut response_json = parse_res.expect("Uncaught error in parse_res for BitbucketAuthInfo");
    save_bitbucket_auth_info_to_db(&mut response_json);
    return Some(response_json);
}

pub async fn refresh_git_auth(review: &Option<Review>) -> Option<String>{
	let authinfo_opt =  bitbucket_auth_info();
    if authinfo_opt.is_none() {
        return None;
    }
    let authinfo = authinfo_opt.expect("empty authinfo_opt in refresh_git_auth");
    let authinfo_opt = update_access_token(&authinfo, review).await;
    if authinfo_opt.is_none() {
        log::error!("[get_access_token_from_bitbucket] Empty authinfo_opt from update_access_token for BitbucketAuthInfo");
        return None;
    }
    let latest_authinfo = authinfo_opt.expect("Empty authinfo_opt");
    let access_token = latest_authinfo.access_token().to_string();
    return Some(access_token);
}

pub async fn update_access_token(auth_info: &BitbucketAuthInfo, review_opt: &Option<Review>) -> Option<BitbucketAuthInfo> {
    let repo_provider = "bitbucket".to_string();
    let now = SystemTime::now();
    let now_secs = now.duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs();
    let timestamp_opt = auth_info.timestamp();
    if timestamp_opt.is_none() {
        log::error!("[update_access_token] No timestamp in BitbucketAuthInfo");
        return None;
    }
    let timestamp = timestamp_opt.expect("Empty timestamp");
    let expires_at = timestamp + auth_info.expires_in();
    if expires_at > now_secs {  
        log::error!("[update_access_token] Not yet expired, expires_at = {}, now_secs = {}", expires_at, now_secs);
        return Some(auth_info.to_owned());
    }
    // auth info has expired
    log::debug!("[update_access_token] auth info expired, expires_at = {}, now_secs = {}", expires_at, now_secs);
    let new_auth_info_opt = bitbucket_refresh_token(auth_info.refresh_token()).await;
    let mut new_auth_info = new_auth_info_opt.clone()
        .expect("empty auhtinfo_opt from update_access_token");
    log::debug!("[update_access_token] New auth info  = {:?}", &new_auth_info);
    let access_token = new_auth_info.access_token().to_string();
    if review_opt.is_some() {
        let review = review_opt.to_owned().expect("Empty review");
        set_git_remote_url(&review, &access_token, &repo_provider);
    }
    save_bitbucket_auth_info_to_db(&mut new_auth_info);
    return new_auth_info_opt;
}

async fn bitbucket_refresh_token(refresh_token: &str) -> Option<BitbucketAuthInfo> {
    let token_url = "https://bitbucket.org/site/oauth2/access_token";
    let client_id = std::env::var("BITBUCKET_CLIENT_ID")
        .expect("BITBUCKET_CLIENT_ID must be set");
    let client_secret = std::env::var("BITBUCKET_CLIENT_SECRET")
        .expect("BITBUCKET_CLIENT_SECRET must be set");
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::CONTENT_TYPE, 
        "application/x-www-form-urlencoded".parse().expect("Invalid content-type"));
    let payload = &[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token)
    ];
    let client = get_client();
    let post_res = client.post(token_url)
        .headers(headers)
        .basic_auth(client_id, Some(client_secret))
        .form(payload)
        .send()
        .await;
    if post_res.is_err() {
        let e = post_res.expect_err("No error in post_err for refres token");
        log::error!("[update_access_token] Unable to get refresh token: {}", e);
        return None;
    }
    let response = post_res.expect("Uncaught error in post_res");
    if !response.status().is_success() {
            log::error!("[update_access_token Failed to get refresh token, status: {} body: {:?}", 
            response.status(), response.text().await);
        return None;
    }
    let parse_res =  response.json().await;
    if parse_res.is_err() {
        let e = parse_res.expect_err("No error in parse_res refresh_token");
        log::error!("[update_access_token] Unable to deserialize refresh token response: {}", e);
        return None;
    }
    let refresh_token_resbody = parse_res.expect("Uncaught error in parse_res");
    return Some(refresh_token_resbody);
}