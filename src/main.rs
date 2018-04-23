#![feature(plugin)]
#![plugin(rocket_codegen)]

extern crate cloudflare;
extern crate dotenv;
extern crate rayon;
extern crate reqwest;
extern crate rocket;
extern crate rocket_contrib;
#[macro_use]
extern crate serde_derive;
extern crate toml;

use cloudflare::Cloudflare;
use rocket::http::RawStr;
use rocket::State;
use rocket::response::Redirect;
use rocket_contrib::{Json, Value};

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

type RedirectMap = RwLock<HashMap<String, String>>;
type CloudflareApi = Mutex<Cloudflare>;

mod errors;
mod redirect_utils;
pub use errors::{Error, Result};

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
struct GithubUserShort {
    name: String,
    email: String,
    username: String,
}

#[derive(Debug, Deserialize)]
struct Commit {
    id: String,
    tree_id: String,
    distinct: bool,
    message: String,
    timestamp: String,
    url: String,
    author: GithubUserShort,
    committer: GithubUserShort,
    added: Vec<String>,
    removed: Vec<String>,
    modified: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PushEvent {
    #[serde(rename = "ref")]
    refs: String,
    before: String,
    after: String,
    compare: String,
    commits: Vec<Commit>,
    head_commit: Commit,
    repository: Value,
    pusher: Value,
    sender: Value,
}

fn redirects_updated(push: PushEvent) -> bool {
    for commit in &push.commits {
        if commit.modified.iter().any(|file| file == "redirects.toml") {
            return true;
        }
    }
    false
}

#[post("/github/webhook", format = "application/json", data = "<hook>")]
fn webhook(
    hook: Json<PushEvent>,
    redirs: State<RedirectMap>,
    cf: State<CloudflareApi>,
) -> Result<&'static str> {
    println!("webhook! {:?}", hook);
    let push: PushEvent = hook.0;

    // check if this is a push to master. if not, return early
    if push.refs != "refs/heads/master" {
        return Ok("Event not on master branch, ignoring");
    }

    // check that the redirects file was actually modified
    if !redirects_updated(push) {
        return Ok("redirects.toml was not modified, ignoring");
    }

    redirect_utils::update_redirect_map(redirs, cf).map(|_| Ok("Redirects Updated!"))?
}

#[get("/")]
fn index(redirs: State<RedirectMap>) -> String {
    let redirs = redirs.read().expect("rlock failed");
    let mut output = String::new();
    for (short, url) in redirs.iter() {
        output.push_str(&format!("{}.rustref.com -> {}\n", short, url));
    }
    output
}

#[get("/redirect/<key>")]
fn redirect_bare(key: String, redirs: State<RedirectMap>) -> Option<Redirect> {
    let map = redirs.read().expect("could not lock rlock");
    match map.get(&key) {
        Some(url) => Some(Redirect::found(url)),
        None => None,
    }
}

#[get("/redirect/<key>/<path>")]
fn redirect(key: String, path: &RawStr, redirs: State<RedirectMap>) -> Option<Redirect> {
    let map = redirs.read().expect("could not lock rlock");
    match map.get(&key) {
        Some(url) => Some(Redirect::found(&format!("{}/{}", url, path))),
        None => None,
    }
}

fn main() {
    let redirects = redirect_utils::redirects_from_file("redirects.toml")
        .expect("error reading redirects from file");

    let cf_api_key: String = dotenv::var("cloudflare_key").expect("no cloudflare key found!");
    let cf_email: String = dotenv::var("cloudflare_email").expect("no cloudflare email found!");
    let cf_api = Cloudflare::new(
        &cf_api_key,
        &cf_email,
        "https://api.cloudflare.com/client/v4/",
    ).expect("failed to create cloudflare client");

    rocket::ignite()
        .mount("/", routes![index, redirect, redirect_bare, webhook])
        .manage(RwLock::new(redirects))
        .manage(Mutex::new(cf_api))
        .launch();
}

#[cfg(test)]
mod tests {
    extern crate serde_json;
    use super::*;

    #[test]
    fn parse_readme_webhook() {
        let json_str = include_str!("../test_data/readme_updated.json");
        let parsed = serde_json::from_str::<PushEvent>(&json_str);
        // println!("{:?}", parsed);
        assert!(parsed.is_ok());
        let push = parsed.unwrap();
        assert!(push.refs == "refs/heads/rocket");
        assert!(push.commits.len() > 0);
        assert!(push.commits[0].modified.len() > 0);
        assert!(push.commits[0].modified[0] == "Readme.md");
        assert!(!redirects_updated(push));
    }

    #[test]
    fn parse_webhook_multiple_commits() {
        let json_str = include_str!("../test_data/multiple_commits.json");
        let parsed = serde_json::from_str::<PushEvent>(&json_str);

        assert!(parsed.is_ok());
        let push = parsed.unwrap();
        assert!(push.refs == "refs/heads/rocket");
        assert!(redirects_updated(push));
    }
}
