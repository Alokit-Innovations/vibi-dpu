use chrono::DateTime;
use jsonwebtoken::{encode, Header, EncodingKey, Algorithm};
use serde::{Deserialize, Serialize};
use std::env;
use std::str;
use chrono::{Utc, Duration};
use std::fs;
use crate::db::github::auth::get_github_auth_info_from_db;
use crate::utils::review::Review;
use crate::utils::user::ProviderEnum;
use crate::{utils::reqwest_client::get_client, utils::github_auth_info::GithubAuthInfo, db::github::auth::save_github_auth_info_to_db};
use crate::utils::gitops::set_git_remote_url;

#[derive(Debug, Serialize, Deserialize)]
struct AccessTokenResponse {
    token: String,
    // Add other fields if necessary
}

#[derive(Debug, Serialize)]
struct Claims {
    iat: i64,
    exp: i64,
    iss: String,
}

fn generate_jwt(github_app_id: &str) -> Option<String> {
    let my_claims = generate_claims(github_app_id);
    let encoding_key_opt = generate_encoding_key();
    if encoding_key_opt.is_none() {
        log::error!("[generate_jwt] Unable to generate encoding key");
        return None;
    }
    let encoding_key = encoding_key_opt.expect("Empty encoding_key_opt");
    let token_res = encode(&Header::new(Algorithm::RS256), &my_claims, &encoding_key);
    if token_res.is_err() {
        let token_err = token_res.expect_err("No error in fetching token");
        log::error!("[generate_jwt] Error encoding JWT: {:?}", token_err);
        return None;
    };
    let token = token_res.expect("Error encoding JWT");
    Some(token)
}

fn generate_claims(github_app_id: &str) -> Claims{
    return Claims {
        iat: Utc::now().timestamp(),
        exp: (Utc::now() + Duration::minutes(5)).timestamp(),
        iss: github_app_id.to_string(),
    };
}

fn generate_encoding_key() -> Option<EncodingKey>{
    let pem_file_path = "/app/repo-profiler.pem";
    let pem_data_res = fs::read(pem_file_path);
    
    if pem_data_res.is_err() {
        let pem_data_err = pem_data_res.expect_err("Empty error in reading pem file");
        log::error!("[generate_encoding_key] Error reading pem file: {:?}", pem_data_err);
        return None;
    }
    let pem_data = pem_data_res.expect("Error reading pem file");
    let encoding_key_res = EncodingKey::from_rsa_pem(&pem_data);
    if encoding_key_res.is_err() {
        let e= encoding_key_res.err().expect("Empty error in encoding_key_res");
        log::error!("[generate_encoding_key] Error creating encoding key: {:?}", e);
        return None;
    }
    let encoding_key = encoding_key_res.expect("Uncaught error in encoding_key_res");
    return Some(encoding_key);
}

pub async fn fetch_access_token(installation_id: &str) -> Option<GithubAuthInfo> {
    let gh_auth_info_opt = call_access_token_api(installation_id).await;
    if gh_auth_info_opt.is_none() {
        log::error!("[fetch_access_token] Unable to get gh auth info");
        return None;
    }
    let mut gh_auth_info = gh_auth_info_opt.expect("Uncaught error in gh_auth_info_opt");
    save_github_auth_info_to_db(&mut gh_auth_info);
    return Some(gh_auth_info);
}

async fn call_access_token_api(installation_id: &str) -> Option<GithubAuthInfo>{
    let github_app_id = env::var("GITHUB_APP_ID");
    let github_app_id_str = github_app_id.expect("GITHUB_APP_ID must be set");
    let jwt_token_opt = generate_jwt(&github_app_id_str);
    if jwt_token_opt.is_none() {
        log::error!("[call_access_token_api] Unable to generate jwt token");
        return None;
    }
    let jwt_token = jwt_token_opt.expect("Empty jwt_token_opt");
    let client = get_client();
    let response = client.post(&format!("https://api.github.com/app/installations/{}/access_tokens", installation_id))
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", format!("Bearer {}", jwt_token))
        .header("User-Agent", "Vibinex code review Test App")
        .send()
        .await;
    if response.is_err() {
        let e = response.expect_err("No error in response");
        log::error!("[call_access_token_api] error in calling github api : {:?}", e);
        return None;
    }
    let response_access_token = response.expect("Uncaught error in reponse");
    if !response_access_token.status().is_success() {
        log::error!(
            "[call_access_token_api] Failed to exchange code for access token. Status code: {}, Response content: {:?}",
            response_access_token.status(),
            response_access_token.text().await
        );
        return None;
    }
    log::debug!("[call_access_token_api] response access token = {:?}", &response_access_token);
    let parse_res = response_access_token.json::<GithubAuthInfo>().await ;
    if parse_res.is_err() {
        let e = parse_res.expect_err("No error in parse_res for AuthInfo");
        log::error!("[call_access_token_api] error deserializing GithubAuthInfo: {:?}", e);
        return None;
    }
    let mut gh_auth_info = parse_res.expect("Uncaught error in parse_res for AuthInfo");
    gh_auth_info.set_installation_id(installation_id);
    return Some(gh_auth_info);
}

async fn get_or_update_auth(review_opt: &Option<Review>) -> Option<GithubAuthInfo> {
	let authinfo_opt =  get_github_auth_info_from_db();
    if authinfo_opt.is_none() {
        return None;
    }
    let auth_info = authinfo_opt.expect("empty authinfo_opt in app_access_token");
    let app_installation_id_opt = auth_info.installation_id().to_owned();
    if app_installation_id_opt.is_none() {
        log::error!("[get_or_update_auth] app_installation_id empty");
        return None;
    }
    let app_installation_id = app_installation_id_opt.expect("Empty app_installation_id_opt");
    if update_condition_satisfied(auth_info.expires_at()) {  
        log::debug!("[get_or_update_auth] access token not yet expired");
        return Some(auth_info.to_owned());
    }
    // auth info has expired
    log::debug!("[get_or_update_auth] github auth info expired");
    let new_auth_info_opt = fetch_access_token(app_installation_id.as_str()).await;
    if new_auth_info_opt.is_none() {
        log::error!("[get_or_update_auth] Unable to fetch access token");
        return None;
    }
    let mut new_auth_info = new_auth_info_opt.clone()
        .expect("empty auhtinfo_opt from get_or_update_auth");
    log::debug!("[get_or_update_auth] New github auth info  = {:?}", &new_auth_info);
    if review_opt.is_some() {
        let review = review_opt.to_owned().expect("Empty review");
        set_git_remote_url(&review, new_auth_info.token(),
            &ProviderEnum::Github.to_string().to_lowercase());
    }
    save_github_auth_info_to_db(&mut new_auth_info);
    return new_auth_info_opt;

}

fn update_condition_satisfied(expires_at: &str) -> bool{
    let now_ts = Utc::now().timestamp();
    let expires_at_dt_res = DateTime::parse_from_rfc3339(expires_at);
    if expires_at_dt_res.is_err() {
        let e = expires_at_dt_res.expect_err("No error in expires_at_dt_res");
        log::error!("[update_condition_satisfied] Unable to parse expires_at to datetime: {:?}", e);
        return false;
    }
    let expires_at_dt = expires_at_dt_res.expect("Uncaught error in expires_at_dt_res");
    let expires_at_ts = expires_at_dt.timestamp();
    return expires_at_ts > now_ts;
}

async fn app_access_token(review: &Option<Review>) -> Option<String>{
    let authinfo_opt = get_or_update_auth(review).await;
    log::debug!("[app_access_token] authinfo_opt = {:?}", &authinfo_opt);
    if authinfo_opt.is_none() {
        log::error!("[app_access_token] Empty latest_authinfo_opt for github auth info");
        return None;
    }
    let authinfo = authinfo_opt.expect("Empty latest_authinfo_opt");
    let access_token = authinfo.token().to_string();
    return Some(access_token);
}

fn pat_access_token() -> Option<String> {
    let github_pat_res: Result<String, env::VarError> = env::var("GITHUB_PAT");
	let provider_res = env::var("PROVIDER");	
	if github_pat_res.is_err() {
		log::debug!("[pat_access_token] GITHUB PAT env var must be set");
        return None;
    }
    let github_pat = github_pat_res.expect("Empty GITHUB_PAT env var");
    if github_pat.len() == 0 {
        log::debug!("[pat_access_token] GITHUB PAT 0 length");
        return None;
    }
    log::debug!("[pat_access_token] GITHUB PAT: [REDACTED], length = {}",
        github_pat.len());
    if provider_res.is_err() {
        log::error!("[pat_access_token] PROVIDER env var must be set");
        return None;
    }
    let provider = provider_res.expect("Empty PROVIDER env var");
    if provider.len() == 0 {
        log::debug!("[pat_access_token] PROVIDER 0 length");
        return None;
    }
    log::debug!("[pat_access_token] PROVIDER: {}", provider);
    if provider.eq_ignore_ascii_case(&ProviderEnum::Github.to_string()) {
        return Some(github_pat);
    }
    return None;
}

pub async fn gh_access_token(review: &Option<Review>) -> Option<String> {
    let pat_token_opt = pat_access_token();
    log::debug!("[gh_access_token] pat_token_opt = {:?}", &pat_token_opt);
    if let Some(pat_token) = pat_token_opt {
        return Some(pat_token);
    }
    return app_access_token(review).await;
}