use std::env;

use reqwest::Response;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{db::repo::save_repo_to_db, utils::reqwest_client::get_client};
use crate::utils::repo::Repository;
use super::config::{github_base_url, get_api_paginated};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserSelectedRepo {
    repo_name: String,
    repo_owner: String,
    repo_provider: String,
}

pub async fn get_github_app_installed_repos(access_token: &str) -> Option<Vec<Repository>> {
    let repos_url = format!("{}/installation/repositories", github_base_url());
    let repos_opt = get_api_paginated(&repos_url, access_token, None).await;
    if repos_opt.is_none() {
        log::error!("[get_github_app_installed_repos] Unable to call get api and get all repos");
        return None;
    }
    let repos_val = repos_opt.expect("Empty repos_opt");
    let repositories = deserialize_repos(repos_val);
    log::debug!("[get_github_app_installed_repos] Fetched {:?} repositories from GitHub", &repositories);
    return Some(repositories)
}

pub async fn get_user_accessed_github_repos(access_token: &str) -> Option<Vec<Repository>> {
    let repos_url = format!("{}/user/repos", github_base_url());
    let repos_opt = get_api_paginated(&repos_url, access_token, None).await;
    if repos_opt.is_none() {
        log::error!("[get_user_accessed_github_repos] Unable to call get api and get all repos");
        return None;
    }
    let repos_val = repos_opt.expect("Empty repos_opt");
    let repositories = deserialise_github_pat_repos(repos_val);
    // filter repositories vec after calling vibinex-server api
    // call vibinex-server api and get selected repo list
    let selected_repositories_opt = user_selected_repos().await;
    if selected_repositories_opt.is_none() {
        log::error!("[get_user_accessed_github_repos] Unable to get repos from server");
        return None;
    }
    let selected_repositories = selected_repositories_opt.expect("EMpty selected_repositories_opt");
    let mut pat_repos: Vec<Repository> = Vec::<Repository>::new();
    // go over all entries in vec and filter them out by repo_name,provider,owner
    for repo in repositories {
        let mut found = false;
        for selected_repo in &selected_repositories {
            if repo.name() == &selected_repo.repo_name && repo.provider() == &selected_repo.repo_provider && repo.owner() == &selected_repo.repo_owner {
                found = true;
                break;
            }
        }
        if found {
            pat_repos.push(repo.clone());
        }
    }
    log::debug!("[get_user_accessed_github_repos] Fetched {:?} repositories from GitHub", &pat_repos);
    return Some(pat_repos)
}

async fn user_selected_repos() -> Option<Vec<UserSelectedRepo>> {
    let client = get_client();
    let topic_name = env::var("INSTALL_ID").expect("INSTALL_ID must be set");
    let provider = env::var("PROVIDER").expect("PROVIDER must be set");
    let server_prefix_url = env::var("SERVER_URL").expect("SERVER_URL must be set");
    let url = format!("{}/api/dpu/repos?topicId={}&provider={}",
			&server_prefix_url, &topic_name, &provider);
    let repos_res = client.get(url).send().await;
    if let Err(e) = repos_res {
        log::error!("[user_selected_repos] Unable to get repos from server, {:?}", e);
        return None;
    }
    let repos_response = repos_res.expect("Uncaught error in repos_res");
    return parse_repos_response(repos_response).await;
}

async fn parse_repos_response(response: Response) -> Option<Vec<UserSelectedRepo>>{
    let repos_value_res = response.json::<Value>().await;
    if let Err(e) = &repos_value_res {
        log::error!("[parse_repos_response] Unable to parse response {:?}", e);
        return None;
    }
    let repos_value = repos_value_res.expect("Uncaught error in repos_values_res");
    let repolist_json_opt = repos_value.get("repoList");
    if repolist_json_opt.is_none() {
        log::error!("[parse_repos_response] No repoList key in response json: {:?}", &repos_value);
        return None;
    }
    let repolist_json = repolist_json_opt.expect("Uncaught error in repolist_json_opt");
    let repolist_vec_res = serde_json::from_value(repolist_json.clone());
    if let Err(e) = &repolist_vec_res {
        log::error!("[parse_repos_response] Unable to parse vec of UserSelectedRepo from response: {:?}", e);
        return None;
    }
    let repolist_vec: Vec<UserSelectedRepo> = repolist_vec_res.expect("Uncaught error in repolist_vec_res");
    return Some(repolist_vec);
}

fn deserialize_repos(repos_val: Vec<Value>) -> Vec<Repository> {
    let mut all_repos = Vec::new();
    for response_json in repos_val {
        let repo_json_opt = response_json["repositories"].as_array();
        if repo_json_opt.is_none() {
            log::error!("[deserialize_repos] Unable to deserialize repo value: {:?}", &response_json);
            continue;
        }
        let repos_page_json = repo_json_opt.expect("Empty repo_json_opt").to_owned();
        for repo_json in repos_page_json {
            let repo = deserialize_repo_object(&repo_json);
            save_repo_to_db(&repo);
            all_repos.push(repo);
        }
    }
    return all_repos;
}

fn deserialise_github_pat_repos(repos_val: Vec<Value>) -> Vec<Repository> {
    let mut all_repos = Vec::new();
    for response_json in repos_val {
        let repo_json_opt = response_json.as_array();
        if repo_json_opt.is_none() {
            log::error!("[deserialize_repos] Unable to deserialize repo value: {:?}", &response_json);
            continue;
        }
        let repos_page_json = repo_json_opt.expect("Empty repo_json_opt").to_owned();
        for repo_json in repos_page_json {
            let repo = deserialize_repo_object(&repo_json);
            save_repo_to_db(&repo);
            all_repos.push(repo);
        }
    }
    return all_repos;
}

fn deserialize_repo_object(repo_json: &Value) -> Repository {
    let is_private_res = repo_json["private"].as_bool();
    let mut is_private = true;
    if is_private_res.is_some() {
        is_private = is_private_res.expect("Uncaught error in is_private_res");
    }
    let repo = Repository::new(
        repo_json["name"].to_string().trim_matches('"').to_string(),
        repo_json["id"].to_string().trim_matches('"').to_string(),
        repo_json["owner"]["login"].to_string().trim_matches('"').to_string(),
        is_private,
        repo_json["ssh_url"].to_string().trim_matches('"').to_string(),
        None,    
        repo_json["owner"]["login"].to_string().trim_matches('"').to_string(),
        None,
        "github".to_string(),
    );
    return repo;
}