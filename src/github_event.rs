use GH_SECRET;

use hmac::{Hmac, Mac};
use sha1::Sha1;
use rocket::data::{self, Data, FromData};
use rocket::http::{ContentType, Status};
use rocket::request::Request;
use rocket::Outcome::{self, *};
use serde_json::{self, Value};

use std::io::Read;

/// Represents a Github user that is passed in by the Github webhook API
#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Serialize)]
pub struct GithubUserShort {
    pub name: String,
    pub email: String,
    pub username: String,
}

/// Represents a Github commit that is passed in by the Github webhook API
#[derive(Debug, Deserialize, Serialize)]
pub struct Commit {
    pub id: String,
    pub tree_id: String,
    pub distinct: bool,
    pub message: String,
    pub timestamp: String,
    pub url: String,
    pub author: GithubUserShort,
    pub committer: GithubUserShort,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

/// Represents a PushEvent that is passed in by the Github webhook API
#[derive(Debug, Deserialize, Serialize)]
pub struct PushEvent {
    #[serde(rename = "ref")]
    pub refs: String,
    pub before: String,
    pub after: String,
    pub compare: String,
    pub commits: Vec<Commit>,
    pub head_commit: Commit,
    pub repository: Value,
    pub pusher: Value,
    pub sender: Value,
}

impl PushEvent {
    /// Checks each modified file in the PushEvent to see if `filename` was modified
    pub fn file_modified(&self, filename: &str) -> bool {
        for commit in &self.commits {
            if commit.modified.iter().any(|f| f == filename) {
                return true;
            }
        }
        false
    }
}

pub struct SignedPushEvent(pub PushEvent);

impl FromData for SignedPushEvent {
    type Error = String;

    fn from_data(req: &Request, data: Data) -> data::Outcome<Self, String> {
        if req.content_type() != Some(&ContentType::JSON) {
            return Outcome::Forward(data);
        }
        let gh_hash = match req.headers().get_one("X-Hub-Signature") {
            Some(h) => h,
            None => return Failure((Status::InternalServerError, "No signature".into())),
        };

        let mut data_str = String::new();
        if let Err(e) = data.open().read_to_string(&mut data_str) {
            return Failure((Status::InternalServerError, format!("{:?}", e)));
        }

        // bail if signature doesn't match
        if generate_github_hash(&GH_SECRET, &data_str) != gh_hash {
            return Failure((Status::Forbidden, "signature mismatch".into()));
        }

        // verified content, parse and return PushEvent
        let event: PushEvent = match serde_json::from_str(&data_str) {
            Ok(ev) => ev,
            Err(e) => return Failure((Status::InternalServerError, format!("{:?}", e))),
        };

        Success(SignedPushEvent(event))
    }
}

pub fn generate_github_hash(secret: &str, json_str: &str) -> String {
    let mut mac = Hmac::<Sha1>::new_varkey(secret.as_bytes()).expect("Hmac creation");
    mac.input(json_str.as_bytes());
    let hmac_result = mac.result().code();

    let mut hash = "sha1=".to_string();

    // hmac produces result as bytes. convert it to a hex string representation
    hash.extend(hmac_result.as_slice().iter().map(|x| format!("{:02x}", x)));
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_hash() {
        // Note: use a securely generated, random secret in production
        let secret = "hello".to_string();
        // an actual payload is the full JSON sent in the request
        let payload = "this is an example payload of what we want to sign.".to_string();
        assert_eq!(
            generate_github_hash(&secret, &payload),
            "sha1=604b8100cfe1aeaee448759c1450f080f41d41db"
        );
    }
}