use crate::db::prs::update_pr_info_in_db;
use crate::utils::{pr_info::PrInfo, reqwest_client::get_client};
use reqwest::header::{HeaderMap, USER_AGENT};
use serde_json::Value;
use std::collections::HashMap;
use std::str;

use super::config::{github_base_url, prepare_headers};


pub async fn list_prs_github(repo_owner: &str, repo_name: &str, access_token: &str, state: &str) -> Option<Vec<String>> {
    let headers_opt = prepare_headers(access_token);
    if headers_opt.is_none() {
        log::error!("[list_prs_github] Unable to prepare auth headers for repository: {}", repo_name);
        return None;
    }
    let headers = headers_opt.expect("Headers should be present");

    let mut params = HashMap::new();
    params.insert("state".to_string(), state.to_string());

    let pr_list_opt = get_list_prs_github(&headers, &params, repo_owner, repo_name).await;
    pr_list_opt
}

async fn get_list_prs_github(headers: &HeaderMap, params: &HashMap<String, String>, repo_owner: &str, repo_name: &str) -> Option<Vec<String>> {
    let client = get_client();
    let base_url = github_base_url();
    let response_result = client
        .get(&format!(
            "{}/repos/{}/{}/pulls",
            &base_url, repo_owner, repo_name
        ))
        .headers(headers.to_owned())
        .query(params)
        .send()
        .await;

    if response_result.is_err() {
        let e = response_result.expect_err("No error in sending request");
        log::error!("[get_list_prs_github] Failed to send the request: {:?}", e);
        return None;
    }

    let response = response_result.expect("Uncaught error in parsing response");
    if !response.status().is_success() {
        log::error!(
            "[get_list_prs_github] Request failed with status: {:?}",
            response.status()
        );
        return None;
    }

    let parse_result = response.json::<Value>().await;
    if parse_result.is_err() {
        let e = parse_result.expect_err("No error in parsing");
        log::error!(
            "[get_list_prs_github] Failed to parse JSON: {:?}",
            e
        );
        return None;
    }
    let prs_data = parse_result.expect("Uncaught error in parsing PRs data");

    let pr_list = prs_data.as_array()
        .expect("Expected an array of PRs")
        .iter()
        .map(|pr| pr["number"].to_string())
        .collect();

    Some(pr_list)
}

pub async fn get_pr_info_github(repo_owner: &str, repo_name: &str, access_token: &str, pr_number: &str) -> Option<PrInfo> {
    let base_url = github_base_url();
    let url = format!(
        "{}/repos/{}/{}/pulls/{}",
        &base_url, repo_owner, repo_name, pr_number
    );
    log::debug!("[get_pr_info_github] URL: {:?}", &url);
    let client = get_client();
    let response_result = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Accept", "application/json")
        .header(USER_AGENT, "Vibinex code review app")
        .send()
        .await;

    if response_result.is_err() {
        let e = response_result.expect_err("No error in getting PR response");
        log::error!("[get_pr_info_github] Error getting PR info: {:?}", e);
        return None;
    }

    let response = response_result.expect("Uncaught error in response");
    if !response.status().is_success() {
        log::error!("[get_pr_info_github] Failed to get PR info, response: {:?}", response.text().await);
        return None;
    }

    let parse_result = response.json::<Value>().await;
    if parse_result.is_err() {
        let e = parse_result.expect_err("No error in parsing");
        log::error!("[get_pr_info_github] Error parsing PR data: {:?}", e);
        return None;
    }

    let pr_data = parse_result.expect("Uncaught error in parsing PR data");

    let pr_info = PrInfo {
        base_head_commit: pr_data["base"]["sha"].as_str()?.to_string(),
        pr_head_commit: pr_data["head"]["sha"].as_str()?.to_string(),
        state: pr_data["state"].as_str()?.to_string(),
        pr_branch: pr_data["head"]["ref"].as_str()?.to_string(),
    };

    log::debug!("[get_pr_info_github] PR info: {:?}", &pr_info);
    Some(pr_info)
}

pub async fn get_and_store_pr_info(repo_owner: &str, repo_name: &str, access_token: &str, pr_number: &str) {
    let repo_provider = "github";
    if let Some(pr_info) = get_pr_info_github(repo_owner, repo_name, access_token, pr_number).await {
        // If PR information is available, store it in the database
        update_pr_info_in_db(repo_owner, repo_name, &pr_info, pr_number, repo_provider).await;
    } else {
        log::error!("[get_and_store_pr_info] No PR info available for PR number: {:?} repository: {:?} repo_owner{:?}", pr_number, repo_name, repo_owner);
    }
}
