#![feature(plugin)]
#![plugin(rocket_codegen)]

extern crate cloudflare;
extern crate dotenv;
extern crate hmac;
#[macro_use]
extern crate lazy_static;
extern crate rayon;
extern crate reqwest;
extern crate rocket;
extern crate rocket_contrib;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate sha1;
extern crate toml;

use cloudflare::Cloudflare;
use rocket::http::RawStr;
use rocket::response::{Redirect, NamedFile};
use rocket::State;
use rocket_contrib::Template;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

mod errors;
mod github_event;
mod redirect_utils;

pub use errors::{Error, Result};
use github_event::{PushEvent, SignedPushEvent};

type RedirectMap = RwLock<RedirectData>;
type CloudflareApi = Mutex<Cloudflare>;

lazy_static! {
    static ref GH_SECRET: String =
        dotenv::var("github_secret").expect("github secret ENV not found!");
}

#[derive(Debug, Serialize)]
pub struct RedirectData {
    map: HashMap<String, String>,
    commit_hash: String,
    commit_url: String,
}

/// Update the servers redirect map whenever `redirects.toml` is updated in the
/// master branch on Github.
///
/// Called by Github's servers whenever there is a `push` event in the Github repository.
/// Returns 200 with message if everything went ok, otherwise a 500 internal error if
/// something went wrong when updating the redirect map
#[post("/github/webhook", data = "<event>")]
fn webhook(
    event: SignedPushEvent,
    redirs: State<RedirectMap>,
    cf: State<CloudflareApi>,
) -> Result<&'static str> {
    let push: PushEvent = event.0;

    // check if this is a push to master. if not, return early
    if push.refs != "refs/heads/master" {
        return Ok("Event not on master branch, ignoring\n");
    }

    // check that the redirects file was actually modified
    if !push.file_modified("redirects.toml") {
        return Ok("redirects.toml was not modified, ignoring\n");
    }

    redirect_utils::update_redirect_map(redirs, cf).map(|_| Ok("Redirects Updated!\n"))?
}

/// Return a page listing all current redirects in alphabetic order
#[get("/")]
fn index(redirs: State<RedirectMap>) -> Template {
    let data: &RedirectData = &*redirs.read().expect("rlock failed");
    Template::render("index", data)
}

/// Redirect a subdomain to its matching page via 302 redirect.
/// If `key` is not in the redirect map return 404.
///
/// Example: cook.rustref.com => https://doc.rust-lang.org/cargo/
#[get("/redirect/<key>")]
fn redirect_bare(key: String, redirs: State<RedirectMap>) -> Option<Redirect> {
    let map: &HashMap<String, String> = &redirs.read().expect("could not lock rlock").map;
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
    let map = &redirs.read().expect("could not lock rlock").map;
    match map.get(&key) {
        Some(url) => Some(Redirect::found(&format!("{}/{}", url, path))),
        None => None,
    }
}

#[get("/<file..>", rank = 2)]
fn files(file: PathBuf) -> Option<NamedFile> {
    NamedFile::open(Path::new("static/").join(file)).ok()
}

fn rocket() -> rocket::Rocket {
    let redirects = redirect_utils::redirects_from_file("redirects.toml")
        .expect("error reading redirects from file");

    let redirect_data = RedirectData {
        map: redirects,
        commit_hash: ".toml".into(),
        commit_url: "".into(),
    };

    let cf_api_key: String = dotenv::var("cloudflare_key").expect("no cloudflare key found!");
    let cf_email: String = dotenv::var("cloudflare_email").expect("no cloudflare email found!");
    let cf_api = Cloudflare::new(
        &cf_api_key,
        &cf_email,
        "https://api.cloudflare.com/client/v4/",
    ).expect("failed to create cloudflare client");

    rocket::ignite()
        .mount("/", routes![index, files, redirect, redirect_bare, webhook])
        .manage(RwLock::new(redirect_data))
        .manage(Mutex::new(cf_api))
        .attach(Template::fairing())
}

fn main() {
    rocket().launch();
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
