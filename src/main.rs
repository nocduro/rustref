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
use cloudflare::zones::dns;
use rayon::prelude::*;
use rocket::http::RawStr;
use rocket::State;
use rocket::response::Redirect;
use rocket_contrib::{Json, Value};

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::io::prelude::*;
use std::sync::{Mutex, RwLock};

type RedirectMap = RwLock<HashMap<String, String>>;
type CloudflareApi = Mutex<Cloudflare>;

mod errors;
use errors::{Error, RedirectError, Result};

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

#[derive(Deserialize)]
struct TomlConfig {
    redirect: Vec<SiteRedirect>,
}

#[derive(Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct SiteRedirect {
    short: String,
    url: String,
}

fn verify_redirects(redirects: &mut [SiteRedirect]) -> Result<()> {
    // verify that we have no duplicate redirect rules
    redirects.sort();
    let mut errors: Vec<RedirectError> = redirects
        .windows(2)
        .filter(|w| w[0].short == w[1].short)
        .map(|w| RedirectError::DuplicateRule(w[0].short.clone()))
        .collect();

    // verify URLs are valid syntactically, and that the URL is online
    errors.extend(
        redirects
            .par_iter()
            .filter_map(|x| check_url(&x.url).err())
            .collect::<Vec<RedirectError>>(),
    );

    if errors.len() > 0 {
        Err(Error::RedirectErrors(errors))
    } else {
        Ok(())
    }
}

/// Verify that `url` is syntactically valid, and that the page is reachable
fn check_url(url: &str) -> std::result::Result<(), RedirectError> {
    let resp = reqwest::get(url).map_err(|e| RedirectError::BadUrl(url.to_string()))?;
    match resp.status().is_success() {
        true => Ok(()),
        false => Err(RedirectError::InvalidPage(format!(
            "{}: {}",
            url,
            resp.status()
        ))),
    }
}

fn update_redirect_map(redirs: State<RedirectMap>, cf: State<CloudflareApi>) -> Result<()> {
    // download new redirect config from github
    println!("downloading updated redirect file...");
    let toml_str = reqwest::get(
        "https://raw.githubusercontent.com/nocduro/rustref/master/redirects.toml",
    )?.text()?;
    let mut new_redirects = toml::from_str::<TomlConfig>(&toml_str)?.redirect;
    verify_redirects(&mut new_redirects)?;
    println!("toml: {}", &toml_str);

    // before setting the new redirects, make sure that cloudflare was updated successfully
    // get current CNAME records:
    let cf_api = cf.lock()?;
    let zone_id = cloudflare::zones::get_zoneid(&cf_api, "nocduro.com")?;
    println!("zone id: {}", &zone_id);
    let cname_records = dns::list_dns_of_type(&cf_api, &zone_id, dns::RecordType::CNAME)?;
    println!("dns: {:#?}", &cname_records);

    {
        let mut redir_map = redirs.write()?;
        *redir_map = vec_redirects_to_hashmap(&new_redirects);
    }

    // overwrite "redirects.toml" so, on next server restart we get the latest config from file??
    Ok(())
}

#[post("/github/webhook", format = "application/json", data = "<hook>")]
fn webhook(hook: Json<PushEvent>, redirs: State<RedirectMap>, cf: State<CloudflareApi>) -> String {
    println!("webhook! {:?}", hook);

    // check if this is a push to master. if not, return early
    update_redirect_map(redirs, cf).expect("update failed :(");

    "ok".to_string()
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

fn vec_redirects_to_hashmap(slice: &[SiteRedirect]) -> HashMap<String, String> {
    let mut map = HashMap::with_capacity(slice.len());
    for redir in slice {
        map.insert(redir.short.clone(), redir.url.clone());
    }
    map
}

fn redirects_from_file<P: AsRef<Path>>(path: P) -> Result<HashMap<String, String>> {
    let mut toml_string = String::new();
    File::open(path.as_ref()).and_then(|mut f| f.read_to_string(&mut toml_string))?;
    let toml_config = toml::from_str::<TomlConfig>(&toml_string)?;
    Ok(vec_redirects_to_hashmap(&toml_config.redirect))
}

fn main() {
    let redirects =
        redirects_from_file("redirects.toml").expect("error reading redirects from file");

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
    }

    #[test]
    fn verify_toml_parses() {
        let toml_str = include_str!("../redirects.toml");
        let parsed = toml::from_str::<TomlConfig>(&toml_str);
        assert!(parsed.is_ok());
        let redir_vec = parsed.unwrap().redirect;
        assert!(redir_vec.len() > 0);
        for redir in &redir_vec {
            assert!(redir.short.len() > 0);
            assert!(redir.url.len() > 0);
        }
    }

    #[test]
    fn verify_production_redirects_valid() {
        let toml_str = include_str!("../redirects.toml");
        let parsed = toml::from_str::<TomlConfig>(&toml_str);
        assert!(parsed.is_ok());
        let mut redir_vec = parsed.unwrap().redirect;
        match verify_redirects(&mut redir_vec) {
            Ok(_) => (),
            Err(Error::RedirectErrors(e)) => {
                let fail_str: String = e.iter().map(|f| format!("{:?}\n", f)).collect();
                panic!(fail_str);
            }
            Err(e) => panic!("invalid redirect error: {:?}", e),
        }
    }

    #[test]
    fn malformed_urls() {
        let bad1 = SiteRedirect {
            short: "bad1".to_string(),
            url: "@#hello/test".to_string(),
        };
        let bad2 = SiteRedirect {
            short: "bad2".to_string(),
            url: "/example.com".to_string(),
        };
        let bad3 = SiteRedirect {
            short: "bad3".to_string(),
            url: "http://example".to_string(),
        };
        let bad4 = SiteRedirect {
            short: "bad4".to_string(),
            url: "test".to_string(),
        };
        let mut vector = vec![bad1, bad2, bad3, bad4];
        match verify_redirects(&mut vector) {
            Ok(_) => panic!("should fail"),
            Err(Error::RedirectErrors(e)) => {
                if e.len() == vector.len() {
                    return;
                }
                let mut fail_str = String::from("only these URLs failed\n");
                for fail in e {
                    fail_str.push_str(&format!("{:?}", fail));
                    fail_str.push('\n');
                }
                panic!(fail_str);
            }
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_duplicate_redirects() {
        let bad1 = SiteRedirect {
            short: "same".to_string(),
            url: "https://nocduro.com".to_string(),
        };
        let bad2 = SiteRedirect {
            short: "same".to_string(),
            url: "https://google.com".to_string(),
        };
        let bad3 = SiteRedirect {
            short: "bad2".to_string(),
            url: "https://google.com".to_string(),
        };
        let mut vector = vec![bad1, bad2, bad3];
        match verify_redirects(&mut vector) {
            Ok(_) => panic!("unexpected pass"),
            Err(Error::RedirectErrors(e)) => {
                if e.len() == 1 {
                    return;
                }
                panic!("Expected 1 failure for the duplicate");
            }
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }

    #[test]
    fn check_url_404() {
        assert!(check_url("https://nocduro.com/invalid_page_name").is_err());
    }

    #[test]
    #[ignore]
    /// this url is valid for some reason!?
    fn check_url_misspell() {
        assert!(check_url("htp://nocduro.com").is_err())
    }

    #[test]
    fn check_url_valid() {
        assert!(check_url("https://nocduro.com/").is_ok());
        assert!(check_url("https://doc.rust-lang.org/").is_ok());
        assert!(check_url("https://doc.rust-lang.org").is_ok());
    }
}
