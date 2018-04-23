use {CloudflareApi, Error, RedirectMap, Result};

use cloudflare;
use cloudflare::zones::dns;
use errors::RedirectError;
use rayon::prelude::*;
use rocket::State;
use reqwest;
use toml;

use std;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

#[derive(Deserialize)]
struct TomlConfig {
    redirect: Vec<SiteRedirect>,
}

#[derive(Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
struct SiteRedirect {
    short: String,
    url: String,
}

pub fn update_redirect_map(redirs: State<RedirectMap>, cf: State<CloudflareApi>) -> Result<()> {
    // download new redirect config from github
    println!("downloading updated redirect file...");
    let toml_str = reqwest::get(
        "https://raw.githubusercontent.com/nocduro/rustref/rocket/redirects.toml",
    )?.text()?;
    let mut new_redirects = toml::from_str::<TomlConfig>(&toml_str)?.redirect;
    verify_redirects(&mut new_redirects)?;

    // before setting the new redirects, make sure that cloudflare was updated successfully
    // get current CNAME records:
    let cf_api = cf.lock()?;
    let zone_id = cloudflare::zones::get_zoneid(&cf_api, "nocduro.com")?;
    println!("zone id: {}", &zone_id);
    let cname_records = dns::list_dns_of_type(&cf_api, &zone_id, dns::RecordType::CNAME)?;
    // println!("dns: {:#?}", &cname_records);

    let cf_errors: Vec<_> = new_redirects
        .iter()
        .filter(|r| {
            // filter out existing redirects that already have CNAME entries
            !cname_records
                .iter()
                .any(|x| x.name == format!("{}.nocduro.com", r.short))
        })
        .map(|new_redir| {
            // create the CNAME record for new redirects
            println!("new redirect: {:?}", new_redir);
            dns::create_proxied_dns_entry(
                &cf_api,
                &zone_id,
                dns::RecordType::CNAME,
                &format!("{}.nocduro.com", new_redir.short),
                "nocduro.com",
            )
        })
        .filter_map(|x| x.err())
        .collect();

    // just print out cloudflare errors for now
    for e in cf_errors {
        println!("Cloudflare error with: {:?}", e)
    }

    // clear Cloudflare's cache

    // update the map, then unlock asap
    {
        let mut redir_map = redirs.write()?;
        *redir_map = vec_redirects_to_hashmap(&new_redirects);
    }

    // TODO: overwrite "redirects.toml" so next server restart we get the latest config from file
    Ok(())
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

    if !errors.is_empty() {
        Err(Error::RedirectErrors(errors))
    } else {
        Ok(())
    }
}

/// Verify that `url` is syntactically valid, and that the page is reachable
fn check_url(url: &str) -> std::result::Result<(), RedirectError> {
    let resp = reqwest::get(url).map_err(|_e| RedirectError::BadUrl(url.to_string()))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(RedirectError::InvalidPage(format!(
            "{}: {}",
            url,
            resp.status()
        )))
    }
}

fn vec_redirects_to_hashmap(slice: &[SiteRedirect]) -> HashMap<String, String> {
    let mut map = HashMap::with_capacity(slice.len());
    for redir in slice {
        map.insert(redir.short.clone(), redir.url.clone());
    }
    map
}

pub fn redirects_from_file<P: AsRef<Path>>(path: P) -> Result<HashMap<String, String>> {
    let mut toml_string = String::new();
    File::open(path.as_ref()).and_then(|mut f| f.read_to_string(&mut toml_string))?;
    let toml_config = toml::from_str::<TomlConfig>(&toml_string)?;
    Ok(vec_redirects_to_hashmap(&toml_config.redirect))
}

#[cfg(test)]
mod tests {
    use super::*;

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
