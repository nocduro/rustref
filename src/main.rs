#![feature(plugin)]
#![plugin(rocket_codegen)]

extern crate cloudflare;
extern crate dotenv;
extern crate reqwest;
extern crate rocket;
extern crate rocket_contrib;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate toml;

use cloudflare::Cloudflare;
use cloudflare::zones::dns;
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
use errors::Result;

#[derive(Debug, Deserialize)]
struct PushEvent {
    #[serde(rename = "ref")]
    refx: String,
    head: String,
    before: String,
    size: u32,
    distinct_size: u32,
    commits: Value, // don't need this
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

fn verify_redirects(redirs: &[SiteRedirect]) -> Result<()> {
    // return the redirects that faild...

    Ok(())
}

fn update_redirect_map(redirs: State<RedirectMap>, cf: State<CloudflareApi>) -> Result<()> {
    // download new redirect config from github
    println!("downloading updated redirect file...");
    let toml_str = reqwest::get(
        "https://raw.githubusercontent.com/nocduro/rustref/master/redirects.toml",
    )?.text()?;
    let new_redirects = toml::from_str::<TomlConfig>(&toml_str)?.redirect;
    verify_redirects(&new_redirects)?;
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

    format!("ok")
}

#[post("/update", format = "application/json", data = "<redirects>")]
fn update(redirects: Json<Vec<SiteRedirect>>, redirs: State<RedirectMap>, cf: State<CloudflareApi>) -> String {
    println!("json is: {:?}", redirects);

    match update_redirect_map(redirs, cf) {
        Ok(_) => println!("redirects updated!"),
        Err(e) => println!("Error with redir: {:?}", e),
    }
    "redirect map updated.\n".to_string()
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
        .mount(
            "/",
            routes![index, redirect, redirect_bare, update, webhook],
        )
        .manage(RwLock::new(redirects))
        .manage(Mutex::new(cf_api))
        .launch();
}
