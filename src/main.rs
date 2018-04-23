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

mod errors;
mod redirect_utils;

pub use errors::{Error, Result};

type RedirectMap = RwLock<HashMap<String, String>>;
type CloudflareApi = Mutex<Cloudflare>;

/// Represents a Github user that is passed in by the Github webhook API
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
struct GithubUserShort {
    name: String,
    email: String,
    username: String,
}

/// Represents a Github commit that is passed in by the Github webhook API
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

/// Represents a PushEvent that is passed in by the Github webhook API
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

/// Checks each modified file in the PushEvent to see if `redirects.toml` was modified
fn redirects_updated(push: &PushEvent) -> bool {
    for commit in &push.commits {
        if commit.modified.iter().any(|file| file == "redirects.toml") {
            return true;
        }
    }
    false
}

/// Update the servers redirect map whenever `redirects.toml` is updated in the
/// master branch on Github.
///
/// Called by Github's servers whenever there is a `push` event in the Github repository.
/// Returns 200 with message if everything went ok, otherwise a 500 internal error if
/// something went wrong when updating the redirect map
#[post("/github/webhook", format = "application/json", data = "<hook>")]
fn webhook(
    hook: Json<PushEvent>,
    redirs: State<RedirectMap>,
    cf: State<CloudflareApi>,
) -> Result<&'static str> {
    let push: PushEvent = hook.0;

    // check if this is a push to master. if not, return early
    if push.refs != "refs/heads/master" {
        return Ok("Event not on master branch, ignoring\n");
    }

    // check that the redirects file was actually modified
    if !redirects_updated(&push) {
        return Ok("redirects.toml was not modified, ignoring\n");
    }

    redirect_utils::update_redirect_map(redirs, cf).map(|_| Ok("Redirects Updated!\n"))?
}

/// Return a page listing all current redirects in alphabetic order
#[get("/")]
fn index(redirs: State<RedirectMap>) -> String {
    let redirs = redirs.read().expect("rlock failed");

    let mut vector: Vec<_> = redirs.iter().collect();
    vector.sort();
    vector
        .iter()
        .map(|(short, url)| format!("{}.rustref.com -> {}\n", short, url))
        .collect()
}

/// Redirect a subdomain to its matching page via 302 redirect.
/// If `key` is not in the redirect map return 404.
///
/// Example: cook.rustref.com => https://doc.rust-lang.org/cargo/
#[get("/redirect/<key>")]
fn redirect_bare(key: String, redirs: State<RedirectMap>) -> Option<Redirect> {
    let map = redirs.read().expect("could not lock rlock");
    match map.get(&key) {
        Some(url) => Some(Redirect::found(url)),
        None => None,
    }
}

/// Redirect a subdomain to its matching page via 302 redirect, preserving path.
/// If `key` is not in the redirect map return 404.
///
/// Example: ex.rustref.com/primitives.html =>
///     https://doc.rust-lang.org/stable/rust-by-example/primitives.html
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
        assert!(!redirects_updated(&push));
    }

    #[test]
    fn parse_webhook_multiple_commits() {
        let json_str = include_str!("../test_data/multiple_commits.json");
        let parsed = serde_json::from_str::<PushEvent>(&json_str);

        assert!(parsed.is_ok());
        let push = parsed.unwrap();
        assert!(push.refs == "refs/heads/master");
        assert!(redirects_updated(&push));
    }
}
