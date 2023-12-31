use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BitbucketAuthInfo {
	access_token: String,
	refresh_token: String,
	expires_in: u64,
	timestamp: Option<u64>,
}

impl BitbucketAuthInfo {

	// Public getter methods
	pub fn access_token(&self) -> &String {
		&self.access_token
	}

	pub fn refresh_token(&self) -> &String {
		&self.refresh_token
	}

	pub fn expires_in(&self) -> u64 {
		self.expires_in
	}

	pub fn timestamp(&self) -> &Option<u64> {
		&self.timestamp
	}

	// Public setters
	pub fn set_timestamp(&mut self, timestamp: u64) {
		self.timestamp = Some(timestamp);
	}
}
