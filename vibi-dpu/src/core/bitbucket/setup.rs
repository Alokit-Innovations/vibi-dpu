use std::env;
use std::str;
use tokio::task;

use crate::bitbucket::auth::get_access_token_from_bitbucket;
use crate::bitbucket::repo::get_workspace_repos;
use crate::bitbucket::workspace::get_bitbucket_workspaces;
use crate::bitbucket::webhook::{get_webhooks_in_repo, add_webhook};
use crate::bitbucket::user::get_and_save_workspace_users;
use crate::bitbucket::prs::{list_prs_bitbucket, get_and_store_pr_info};
use crate::core::utils::send_aliases;
use crate::core::utils::user_selected_repos;
use crate::core::utils::UserSelectedRepo;
use crate::db::webhook::save_webhook_to_db;
use crate::utils::gitops::get_git_aliases;
use crate::utils::repo::Repository;
use crate::utils::setup_info::SetupInfo;
use crate::utils::gitops::clone_git_repo;
use crate::core::utils::send_setup_info;
use crate::utils::user::ProviderEnum;

pub async fn handle_install_bitbucket(installation_code: &str) {
	// get access token from installation code by calling relevant repo provider's api
	// out of github, bitbucket, gitlab
	let repo_provider = "bitbucket";
	let authinfo_opt = get_access_token_from_bitbucket(installation_code).await;
	if authinfo_opt.is_none() {
		log::error!("[handle_install_bitbucket] Unable to get authinfo in get_access_token_from_bitbucket");
		return;
	}
	let authinfo = authinfo_opt.expect("Empty authinfo_opt");
	log::debug!("[handle_install_bitbucket] AuthInfo: {:?}", authinfo);
	let access_token = authinfo.access_token().clone();
	let user_selected_repos_opt = user_selected_repos(&ProviderEnum::Bitbucket.to_string()).await;
	let user_workspaces = get_bitbucket_workspaces(&access_token).await;
	let mut pubreqs: Vec<SetupInfo> = Vec::new();
	for workspace in user_workspaces {
		let workspace_slug = workspace.slug();
		log::debug!("[handle_install_bitbucket] Processing workspace: {:?}", workspace_slug);
	
		get_and_save_workspace_users(workspace.uuid(), &access_token).await;
		let filtered_repos_opt = filter_user_selected_repos(workspace_slug.to_string(), user_selected_repos_opt.clone());
		if filtered_repos_opt.is_none() {
			log::error!("[handle_install_bitbucket] No repos found for workspace {}", workspace_slug);
			continue;
		}
		let filtered_repos = filtered_repos_opt.expect("Empty filtered_repos_opt");
		let mut reponames: Vec<String> = Vec::new();
		for repo in filtered_repos {
			let token_copy = access_token.clone();
			let mut repo_copy = repo.clone();
			clone_git_repo(&mut repo_copy, &token_copy, &repo_provider).await;
			let aliases_opt = get_git_aliases(&repo_copy);
			if aliases_opt.is_none() {
				log::error!("[handle_install_bitbucket] No aliases for repo {}", repo.name());
				continue;
			}
			let aliases = aliases_opt.expect("Empty aliases_opt");
			log::debug!("[handle_install_bitbucket] aliases for repo: {}/{}: {:?}",
				&repo.owner(), &repo.name(), &aliases);
			send_aliases(&repo, &aliases).await;
			let repo_name = repo.name();
			reponames.push(repo_name.clone());
			log::debug!("[handle_install_bitbucket] Repo url git = {:?}", &repo.clone_ssh_url());
			log::debug!("[handle_install_bitbucket] Repo name = {:?}", repo_name);
			process_webhooks(workspace_slug.to_string(),
			repo_name.to_string(),
			access_token.to_string()).await;
			let repo_name_async = repo_name.clone();
			let workspace_slug_async = workspace_slug.clone();
			let access_token_async = access_token.clone();
			task::spawn(async move {
				let pr_list_opt = list_prs_bitbucket(&workspace_slug_async, &repo_name_async, &access_token_async, "OPEN").await;
				if pr_list_opt.is_none() {
					log::debug!("[handle_install_bitbucket] No open pull requests found for processing.");
					return;
				}
				let pr_list = pr_list_opt.expect("Empty pr_list_opt");
				// We can concurrently process each PR with tokio::spawn.
				for pr_id in pr_list.iter() {
					let workspace_slug_async = workspace_slug_async.clone(); //Instead of cloning each time, I could have used ARC but not sure what is the best way.
					let repo_name_async = repo_name_async.clone();
					let access_token_async = access_token_async.clone();
					let pr_id_async = pr_id.clone();
					task::spawn(async move {
						get_and_store_pr_info(&workspace_slug_async, &repo_name_async, &access_token_async, &pr_id_async).await;
					});
				}          
			});
		}
		pubreqs.push(SetupInfo {
			provider: "bitbucket".to_owned(),
			owner: workspace_slug.clone(),
			repos: reponames
		});
	}
	log::debug!("[handle_install_bitbucket] SetupInfo: {:?}", &pubreqs);
	send_setup_info(&pubreqs).await;
}

fn filter_user_selected_repos(workspace_slug: String, user_selected_repos_opt: Option<Vec<Repository>>) -> Option<Vec<Repository>> {
	if user_selected_repos_opt.as_ref().map_or(true,
		|user_repos| user_repos.is_empty()) {
		log::error!("[filter_user_selected_repos] No user selected repos found");
		return None;
	}
	let user_selected_repos = user_selected_repos_opt.expect("Empty user selected repos");
	let filtered_repos: Vec<Repository> = user_selected_repos.into_iter().filter(|repo| {
			repo.owner() == &workspace_slug
	})
	.collect();
	return Some(filtered_repos);
}

async fn process_webhooks(workspace_slug: String, repo_name: String, access_token: String) {
	log::info!("Setting up webhooks for {}/{}...", &workspace_slug, repo_name);
	let webhooks_data = get_webhooks_in_repo(&workspace_slug, &repo_name, &access_token).await;
	let webhook_callback_url = format!("{}/api/bitbucket/callbacks/webhook", 
		env::var("SERVER_URL").expect("SERVER_URL must be set"));
	if webhooks_data.is_empty() {
		let repo_name_async = repo_name.clone();
		let workspace_slug_async = workspace_slug.clone();
		let access_token_async = access_token.clone();
		task::spawn(async move {
			log::info!("Adding new webhook for {}/{}...",
				&workspace_slug_async, access_token_async);
			add_webhook(
				&workspace_slug_async, 
				&repo_name_async, 
				&access_token_async).await;
		});
		return;
	}
	let matching_webhook = webhooks_data.into_iter()
		.find(|w| w.url().to_string() == webhook_callback_url);
	if matching_webhook.is_none() {
		let repo_name_async = repo_name.clone();
		let workspace_slug_async = workspace_slug.clone();
		let access_token_async = access_token.clone();
		task::spawn(async move {
			log::info!("Adding new webhook for {}/{}...",
				&workspace_slug_async, access_token_async);
			add_webhook(
				&workspace_slug_async, 
				&repo_name_async, 
				&access_token_async).await;
		});
		return;
	}
	let webhook = matching_webhook.expect("no matching webhook");
	log::info!("Not adding webhook, already exists: {:?}...", webhook.url());
	save_webhook_to_db(&webhook);
}