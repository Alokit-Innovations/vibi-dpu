use crate::db::repo::save_repo_to_db;
use crate::utils::repo::Repository;
use super::config::{github_base_url, get_api_values};

pub async fn get_github_app_installed_repos(access_token: &str) -> Option<Vec<Repository>> {
    let repos_url = format!("{}/installation/repositories", github_base_url());
    let repositories = get_api_values(&repos_url, access_token, None).await;
    let mut repos_data = Vec::new();
    for repo_json in repositories {
        let is_private_res = repo_json["private"].as_bool();
        let mut is_private = true;
        if is_private_res.is_some() {
            is_private = is_private_res.expect("Uncaught error in is_private_res");
        }
        let val = Repository::new(
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
        save_repo_to_db(&val);
        repos_data.push(val);
    }
    println!("Fetched {:?} repositories from GitHub", repos_data);
    Some(repos_data)
}