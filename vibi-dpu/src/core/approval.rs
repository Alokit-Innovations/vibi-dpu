use serde_json::Value;

use crate::core::utils::get_access_token;
use crate::db::repo_config::save_repo_config_to_db;
use crate::utils::coverage::CoverageMap;
use crate::github;
use crate::core;
use crate::utils::relevance::Relevance;
use crate::utils::repo_config::RepoConfig;
use crate::{db::review::get_review_from_db, utils::user::ProviderEnum};


pub async fn process_approval(deserialised_msg_data: &Value, repo_config_val: &Value,
        repo_owner: &str, repo_name: &str, pr_number: &str, repo_provider: &str) {
    log::debug!("[process_approval] processing approval msg - {:?}", deserialised_msg_data);
    let repo_config_res = serde_json::from_value(repo_config_val.to_owned());
    let repo_config: RepoConfig;
	if let Err(e) = &repo_config_res {
		log::error!("[process_approval] Unable to deserialze repo_config_res: {:?}", e);
		repo_config = RepoConfig::default();
	} else {
        repo_config = repo_config_res.expect("Uncaught error in repo_config_res");
    }
	log::debug!("[process_approval] repo_config = {:?}", &repo_config);
	save_repo_config_to_db(&repo_config, repo_name, repo_owner, repo_provider);
    if !repo_config.comment() {
        log::info!("[process_approval] Comment setting is turned off");
        return;
    }
    let pr_head_commit = deserialised_msg_data["review"]["commit_id"]
        .to_string().trim_matches('"').to_string();
    let review_opt = get_review_from_db(&repo_name,
        &repo_owner, &repo_provider, &pr_number);
    if review_opt.is_none() {
        log::error!("[process_approval] Unable to get review from db");
        return;
    }
    let review = review_opt.expect("Empty review_opt");

    let access_token= get_access_token(&Some(review.clone()), &repo_provider).await;
	if access_token.is_none(){
		log::error!("[process_approval] no final access token opt");
		return;
	}
	let final_access_token = access_token.expect("Empty final access token opt");

    // get reviewer login array by getting pr all reviewer info from gh/bb
    let mut reviewer_handles = Vec::<String>::new();
    if repo_provider == ProviderEnum::Github.to_string().to_lowercase() {
        let reviewer_handles_opt = github::prs::pr_reviewer_handles(
            &repo_owner, &repo_name, &pr_number, &pr_head_commit, &final_access_token).await;
        if reviewer_handles_opt.is_none(){
            log::error!("[process_approval] no reviewers handles opt");
            return;
        }
        reviewer_handles = reviewer_handles_opt.expect("Empty reviewer_handles_opt");

    } //TODO: create a similar function for bitbucket too
    // get coverage map aliases and their corresponding logins from db/server
    let relevance_vec_opt = review.relevance();
    if relevance_vec_opt.is_none() {
        log::error!("[process_approval] Unable to get coverage from db");
        return;
    }
    let relevance_vec = relevance_vec_opt.to_owned().expect("Empty coverage_opt");
    let mut coverage_map_obj = CoverageMap::new(repo_provider.to_string());
    coverage_map_obj.calculate_coverage_map(relevance_vec.clone(), reviewer_handles.clone());
    // add up contribution of aliases
    // add comment
    let comment_text = approval_comment_text(&coverage_map_obj, relevance_vec, reviewer_handles);
    // get access token and call add_comment in gh/bb
    core::github::comment::add_comment(&comment_text, &review, &final_access_token).await;
}

fn approval_comment_text(coverage_map: &CoverageMap, relevance_vec: Vec<Relevance>, reviewer_handles: Vec<String>) -> String {
    let mut comment = "Relevant users for this PR:\n\n".to_string();  // Added two newlines
    let coverage_text = coverage_map.generate_coverage_table(relevance_vec, reviewer_handles);
    comment += &coverage_text;
    comment += "\n\n";
    comment += "If you are a relevant reviewer, you can use the [Vibinex browser extension](https://chromewebstore.google.com/detail/vibinex-code-review/jafgelpkkkopeaefadkdjcmnicgpcncc) to see parts of the PR relevant to you\n";  // Added a newline at the end
    comment += "Relevance of the reviewer is calculated based on the git blame information of the PR. To know more, hit us up at contact@vibinex.com.\n\n";  // Added two newlines
    comment += "To change comment and auto-assign settings, go to [your Vibinex repository settings page.](https://vibinex.com/u)\n";  // Added a newline at the end
    return comment;
}
