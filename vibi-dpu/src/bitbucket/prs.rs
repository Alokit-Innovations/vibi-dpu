use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;
use std::collections::HashMap;
use std::str;
use std::env;
use crate::client::config::get_client;
use crate::utils::prInfo::prInfo;
use crate::db::prs::save_pr_info_to_db;

pub async fn list_prs_bitbucket(repo_owner: &str, repo_name: &str, access_token: &str, state: &str) -> Vec<u32> {
    let mut pr_list = Vec::new();
    let client = get_client();

    let mut headers = HeaderMap::new();
    headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", access_token)).unwrap());
    
    let mut params = HashMap::new();
    params.insert("state".to_string(), state.to_string());

    let response_result = client
        .get(&format!("{}/repositories/{}/{}/pullrequests", env::var("SERVER_URL").expect("SERVER_URL must be set"), repo_owner, repo_name))
        .headers(headers)
        .query(&params)
        .send()
        .await;

    if response_result.is_err() {
        let e = response_result.expect_err("No error in sending request");
        eprintln!("Failed to send the request {:?}", e);
        return pr_list;
    }

    let response = response_result.expect("Uncaught error in parsing response");

    if response.status() != StatusCode::OK {
        eprintln!("Request failed with status: {:?}", response.status());
        return pr_list;
    }

    let parse_result = response.json::<Value>().await;
    if parse_result.is_err() {
        let parse_result_err = parse_result.expect_err("No error in parsing");
        eprintln!("Failed to parse JSON: {:?}", parse_result_err);
        return pr_list;
    }

    let prs_data = parse_result.expect("Uncaught error in parsing Prs data");

    if let Value::Array(pull_requests) = prs_data["values"].clone() {
        for pr in pull_requests.iter() {
            if let Some(id) = pr["id"].as_u64() {
                pr_list.push(id as u32);
            }
        }
    }
    pr_list
}



pub async fn get_pr_info(workspace_slug: &str, repo_slug: &str, access_token: &str, pr_number: &str) -> Option<prInfo> {
    let url = format!("{}/repositories/{}/{}/pullrequests/{}", env::var("SERVER_URL").expect("SERVER_URL must be set"), workspace_slug, repo_slug, pr_number);

    let client = get_client();
    let response_result = client.get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Accept", "application/json")
        .send()
        .await;
  
    if response_result.is_err() {
        let res_err = response_result.expect_err("No error in getting Pr response");
        println!("Error getting PR info: {:?}", res_err);
        return None;
    }
    let response = response_result.expect("Uncaught error in response");
    if !response.status().is_success() {
        println!("Failed to get PR info, status: {:?}", response.status());
        return None;
    }
    let pr_data: Value = response.json().await.unwrap_or_default();

    Some(prInfo {
        base_head_commit: pr_data["destination"]["commit"]["hash"].as_str().unwrap_or_default().to_string(),
        pr_head_commit: pr_data["source"]["commit"]["hash"].as_str().unwrap_or_default().to_string(),
        state: pr_data["state"].as_str().unwrap_or_default().to_string(),
        pr_branch: pr_data["source"]["branch"]["name"].as_str().unwrap_or_default().to_string(),
    })
}

pub async fn get_and_store_pr_info(workspace_slug: &str,repo_slug: &str,access_token: &str, pr_number: &str) {
    if let Some(pr_info) = get_pr_info(workspace_slug, repo_slug, access_token, pr_number).await {
        // If PR information is available, store it in the database
       save_pr_info_to_db(workspace_slug, repo_slug, pr_info, pr_number).await;
    } else {
        eprintln!("No PR info available for PR number: {:?} repository: {:?} repo_owner{:?}", pr_number, repo_slug, workspace_slug);
    }
}