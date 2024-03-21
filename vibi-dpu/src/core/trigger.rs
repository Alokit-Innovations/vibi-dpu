use serde_json::Value;

use crate::{db::{repo_config::save_repo_config_to_db, review::get_review_from_db}, utils::{repo_config::RepoConfig, review::Review}};

pub async fn process_trigger(message_data: &Vec<u8>) {
    // parse message
    let parse_res = parse_trigger_msg(message_data);
    // create review object from db
    if parse_res.is_none() {
        log::error!("[process_trigger] Unable to parse message: {:?}", &message_data);
        return;
    }
    let (review, repo_config) = parse_res.expect("Empty parse_res");
    // get access token
    // get pr information and update review object
    // commit_check
    // process_review_changes
    // send_hunkmap
}


fn parse_trigger_msg(message_data: &Vec<u8>) -> Option<(Review, RepoConfig)> {
	let data_res = serde_json::from_slice::<Value>(&message_data);
	if data_res.is_err() {
		let e = data_res.expect_err("No error in data_res");
		log::error!("[parse_trigger_msg] Incoming message does not contain valid triggers: {:?}", e);
		return None;
	}
	let deserialized_data = data_res.expect("Uncaught error in deserializing message_data");
	log::debug!("[parse_trigger_msg] deserialized_data == {:?}", &deserialized_data);
	let repo_provider = deserialized_data["repo_provider"].to_string().trim_matches('"').to_string();
    let repo_owner = deserialized_data["repo_owner"].to_string().trim_matches('"').to_string();
    let repo_name = deserialized_data["repo_name"].to_string().trim_matches('"').to_string();
    let pr_number = deserialized_data["pr_number"].to_string().trim_matches('"').to_string();
    let repo_config_res = serde_json::from_value(deserialized_data["repo_config"].clone());
    if let Err(e) = &repo_config_res {
        log::error!("[parse_trigger_msg] Error in parsing repo config: {:?}", e);
        return None;
    }
    let repo_config: RepoConfig = repo_config_res.expect("Uncaught error in repo_config_res");
    let review_opt = get_review_from_db(&repo_name, &repo_owner,
        &repo_provider, &pr_number);
    if review_opt.is_none() {
        log::error!("[parse_trigger_msg] Unable to get review from db: {:?}", &deserialized_data);
        return None;
    }
    let review = review_opt.expect("Empty review_opt");
	let repo_config_res = serde_json::from_value(deserialized_data["repo_config"].clone());
	if repo_config_res.is_err() {
		let e = repo_config_res.expect_err("No error in repo_config_res");
		log::error!("[parse_review] Unable to deserialze repo_config_res: {:?}", e);
		let default_config = RepoConfig::default();
		return Some((review, default_config));
	}
	let repo_config = repo_config_res.expect("Uncaught error in repo_config_res");
	log::debug!("[parse_review] repo_config = {:?}", &repo_config);
	save_repo_config_to_db(&repo_config, &review.repo_name(), &review.repo_owner(), &review.provider());
	return Some((review, repo_config));
}