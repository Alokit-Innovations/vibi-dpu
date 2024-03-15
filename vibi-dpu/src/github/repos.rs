use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::repo::save_repo_to_db;
use crate::utils::repo::Repository;
use super::config::{github_base_url, get_api_paginated};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserSelectedRepo {
    name: String,
    owner: String,
    provider: String,
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
    let selected_repositories: Vec<UserSelectedRepo>; //comes from server api
    let mut pat_repos: Vec<Repository> = Vec::<Repository>::new();
    // go over all entries in vec and filter them out by repo_name,provider,owner
    for repo in repositories {
        let mut found = false;
        for selected_repo in selected_repositories {
            if repo.name() == &selected_repo.name && repo.provider() == &selected_repo.provider && repo.owner() == &selected_repo.owner {
                found = true;
                break;
            }
        }
        if found {
            pat_repos.push(repo.clone());
            break;
        }
    }
    log::debug!("[get_user_accessed_github_repos] Fetched {:?} repositories from GitHub", &pat_repos);
    return Some(pat_repos)
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