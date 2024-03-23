use crate::core::trigger::process_trigger;
use crate::{core::bitbucket::setup::handle_install_bitbucket, utils::user::ProviderEnum};
use crate::core::github::setup::handle_install_github;
use crate::core::review::process_review;
use crate::db::prs::{bitbucket_process_and_update_pr_if_different, github_process_and_update_pr_if_different};
use futures_util::StreamExt;
use google_cloud_auth::credentials::CredentialsFile;
use google_cloud_default::WithAuthExt;
use google_cloud_pubsub::{
    client::{Client, ClientConfig},
    subscription::{Subscription, SubscriptionConfig},
};
use serde::Deserialize;
use serde_json::Value;
use sha256::digest;
use std::collections::HashMap;
use std::collections::VecDeque;
use tokio::task;
use tonic::Code;

#[derive(Debug, Deserialize)]
struct InstallCallback {
    repository_provider: String,
    installation_code: String,
}

async fn process_message(attributes: &HashMap<String, String>, data_bytes: &Vec<u8>) {
    let msgtype_opt = attributes.get("msgtype");
    if msgtype_opt.is_none() {
        log::error!("[process_message] msgtype attribute not found in message, attr: {:?}", attributes);
        return;
    }
    let msgtype = msgtype_opt.expect("Empty msgtype");
    match msgtype.as_str() {
        "install_callback" => {
            process_install_callback(&data_bytes).await;
        }
        "webhook_callback" => {
            let data_bytes_async = data_bytes.to_owned();
            let deserialized_data_opt = deserialized_data(&data_bytes_async);
            let deserialised_msg_data = deserialized_data_opt.expect("Failed to deserialize data");
            
            log::debug!("[process_message] [webhook_callback | deserialised_msg data] {} ", deserialised_msg_data);
            let is_reviewable = process_and_update_pr_if_different(&deserialised_msg_data).await;
            if is_reviewable {
                task::spawn(async move {
                    process_review(&data_bytes_async).await;
                    log::info!("[process_message] Webhook callback message processed!");
                });
            }
        }
        "manual_trigger" => {
            process_trigger(&data_bytes).await;
        }
        _ => {
            log::error!("[process_message] Message type not found for message : {:?}", attributes);
        }
    };
}

async fn process_install_callback(data_bytes: &[u8]) {
    log::info!("[process_install_callback] Processing installation callback message");
    let msg_data_res = serde_json::from_slice::<InstallCallback>(data_bytes);
    if msg_data_res.is_err() {
        log::error!("[process_install_callback] Error deserializing install callback: {:?}", msg_data_res);
        return;
    }
    let data = msg_data_res.expect("msg_data not found");
    if data.repository_provider == ProviderEnum::Github.to_string().to_lowercase() {
        let code_async = data.installation_code.clone();
        task::spawn(async move {
            handle_install_github(&code_async).await;
            log::info!("[process_install_callback] Github installation callback processed");
        });
    }
    if data.repository_provider == ProviderEnum::Bitbucket.to_string().to_lowercase() {
        let code_async = data.installation_code.clone();
        task::spawn(async move {
            handle_install_bitbucket(&code_async).await;
            log::info!("[process_install_callback] Bitbucket installation callback processed");
        });
    }
}

pub async fn get_pubsub_client_config(keypath: &str) -> ClientConfig {
    let credfile = CredentialsFile::new_from_file(keypath.to_string())
        .await
        .expect("Failed to locate credentials file");
    return ClientConfig::default()
        .with_credentials(credfile)
        .await
        .expect("Unable to get PubSub Client config");
}

async fn setup_subscription(keypath: &str, topicname: &str) -> Subscription {
    let config = get_pubsub_client_config(keypath).await;
    let client = Client::new(config)
        .await
        .expect("Unable to create pubsub client to listen to messages");
    let topic = client.topic(topicname);
    let topic_res = topic.exists(None).await;
    if topic_res.is_err() {
        let e = topic_res.expect_err("No error found in topic_res");
        if e.code() == Code::NotFound {
            client
                .create_topic(topicname, None, None)
                .await
                .expect("Unable to create topic");
        } else {
            log::error!("[setup_subscription] Error getting topic: {:?}", e);
        }
    }
    let sub_config = SubscriptionConfig {
        enable_message_ordering: true,
        ..Default::default()
    };
    let subscription_name = format!("{topicname}-sub");
    let subscription = client.subscription(&subscription_name);
    if !subscription
        .exists(None)
        .await
        .expect("Unable to get subscription information")
    {
        subscription
            .create(topic.fully_qualified_name(), sub_config, None)
            .await
            .expect("Unable to create subscription for listening to messages");
    }
    log::debug!("[setup_subscription] sub = {:?}", &subscription);
    subscription
}

pub async fn listen_messages(keypath: &str, topicname: &str) {
    let queue_cap = 100;
    let mut message_hashes = VecDeque::with_capacity(queue_cap);
    let subscription = setup_subscription(keypath, topicname).await;
    let mut stream = subscription
        .subscribe(None)
        .await
        .expect("Unable to subscribe to messages");
    while let Some(message) = stream.next().await {
        log::info!("[listen_messages] Listening for messages...");
        let attrmap: HashMap<String, String> =
            message.message.attributes.clone().into_iter().collect();
        let message_hash = digest(&*message.message.data);
        if !message_hashes.contains(&message_hash) {
            message_hashes.push_back(message_hash);
            if message_hashes.len() > queue_cap {
                while message_hashes.len() > queue_cap {
                    message_hashes.pop_front();
                }
            }
            let msg_bytes = message.message.data.clone();
            process_message(&attrmap, &msg_bytes).await;
        }
        // Ack or Nack message.
        let _ = message.ack().await;
    }
}

pub fn deserialized_data(message_data: &Vec<u8>) -> Option<Value> {
    let msg_data_res = serde_json::from_slice::<Value>(message_data);
    if msg_data_res.is_err() {
        let e = msg_data_res.expect_err("No error in data_res");
        log::error!("[deserialized_data] Incoming message does not contain valid reviews: {:?}", e);
        return None;
    }
    let deserialized_data = msg_data_res.expect("Uncaught error in deserializing message_data");
    log::debug!(
        "[deserialized_data] deserialized_data == {:?}",
        &deserialized_data["eventPayload"]["repository"]
    );
    Some(deserialized_data)
}

async fn process_and_update_pr_if_different(deserialised_msg_data: &Value) -> bool {
    log::debug!("[process_webhook_callback] {}", deserialised_msg_data);
    let repo_provider = deserialised_msg_data["repositoryProvider"].to_string().trim_matches('"').to_string();
    let mut is_reviewable = false;
    if repo_provider == ProviderEnum::Github.to_string().to_lowercase() {
        let repo_owner = deserialised_msg_data["eventPayload"]["repository"]["owner"]["login"].to_string().trim_matches('"').to_string();
        let repo_name = deserialised_msg_data["eventPayload"]["repository"]["name"].to_string().trim_matches('"').to_string();
        let pr_number = deserialised_msg_data["eventPayload"]["pull_request"]["number"].to_string().trim_matches('"').to_string();
        let event_type = deserialised_msg_data["eventType"].to_string().trim_matches('"').to_string();

        log::debug!("[process_webhook_callback] {}, {}, {}, {}", event_type, repo_owner, repo_name, pr_number);
        if event_type == "pull_request_review" {
			log::info!("[process_webhook_callback] Github PR review event");
			is_reviewable = github_process_and_update_pr_if_different(&deserialised_msg_data["eventPayload"], &deserialised_msg_data["repoConfig"], &repo_owner, &repo_name, &pr_number, &repo_provider).await;
        }
        if event_type == "pull_request" {
            log::info!("[process_webhook_callback] Github PR opened");
            is_reviewable = github_process_and_update_pr_if_different(&deserialised_msg_data["eventPayload"], &deserialised_msg_data["repoConfig"], &repo_owner, &repo_name, &pr_number, &repo_provider).await;
        }
    }
    if repo_provider == ProviderEnum::Bitbucket.to_string().to_lowercase() {
        let workspace_slug = deserialised_msg_data["eventPayload"]["repository"]["workspace"]["slug"].to_string().trim_matches('"').to_string();
        let repo_slug = deserialised_msg_data["eventPayload"]["repository"]["name"].to_string().trim_matches('"').to_string();
        let pr_number = deserialised_msg_data["eventPayload"]["pullrequest"]["id"].to_string().trim_matches('"').to_string();
        let event_type = deserialised_msg_data["eventType"].to_string().trim_matches('"').to_string();
        let if_process_pr = bitbucket_process_and_update_pr_if_different(&deserialised_msg_data["eventPayload"],&workspace_slug,&repo_slug,&pr_number,&repo_provider,
        )
        .await;

        if event_type == "pullrequest:approved" {
            is_reviewable = false;
            todo!("Process approved event");
        };
        if if_process_pr && (event_type == "pullrequest:created" || event_type == "pullrequest:updated") {
            is_reviewable = true;
        };
    };
    return is_reviewable;

}