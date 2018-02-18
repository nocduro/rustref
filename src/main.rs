extern crate rayon;
extern crate reqwest;
#[macro_use]
extern crate serde_derive;
extern crate toml;

use std::fs::File;
use std::env;
use std::io;
use std::io::prelude::*;
use std::io::BufWriter;
use std::path::Path;

use rayon::prelude::*;

#[derive(Deserialize)]
struct Config {
    redirect: Vec<Redirect>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
struct Redirect {
    short: String,
    url: String,
}

#[derive(Debug)]
enum Error {
    UrlParseError(String),
    UrlStatusError(String),
}

fn main() {
    let mut args = env::args();
    let mut input = String::new();
    if args.len() > 1 {
        let name = args.nth(1).unwrap();
        File::open(&name)
            .and_then(|mut f| f.read_to_string(&mut input))
            .unwrap();
    } else {
        io::stdin().read_to_string(&mut input).unwrap();
    }

    match toml::from_str::<Config>(&input) {
        Ok(ref mut toml) => generate_redirects(&mut toml.redirect, "_redirects"),
        Err(err) => println!("Failed to parse redirects: {}", err),
    }
}

fn generate_redirects<P: AsRef<Path>>(redirects: &mut [Redirect], output_path: P) {
    // verify that we have no duplicate redirect rules    
    redirects.sort();
    let dupes: Vec<_> = redirects
        .windows(2)
        .filter(|w| w[0] == w[1])
        .map(|w| w[0].short.clone())
        .collect();
    
    if dupes.len() != 0 {
        eprintln!("Error: duplicate redirect rules for:");
        dupes.iter().for_each(|f| eprintln!("\t{}", f));
        ::std::process::exit(1);
    }

    // verify URLs are valid syntactically, and that the URL is online
    let failures: Vec<_> = redirects
        .par_iter()
        .filter_map(|x| check_url(&x.url).err())
        .collect();

    if failures.len() != 0 {
        failures.iter().for_each(|f| eprintln!("{:?}", f));
        eprintln!("Redirects not generated");
        ::std::process::exit(1);
    }

    // generate the netlify redirect file
    let netlify = File::create(output_path.as_ref()).expect("Unable to create file");
    let mut netlify = BufWriter::new(netlify);
    let netlify_string: String = redirects
        .iter()
        .map(|r| format!("https://{}.rustref.com/* {} 301!\n", r.short, r.url))
        .collect();

    netlify
        .write_all(netlify_string.as_bytes())
        .expect("Unable to write data");

    // generate the homepage
    redirects.iter().for_each(|r| println!("{:?}", r));
}

/// Verify that `url` is syntactically valid, and that the page is reachable
fn check_url(url: &str) -> Result<(), Error> {
    let resp = reqwest::get(url).map_err(|e| Error::UrlParseError(e.to_string()))?;
    match resp.status().is_success() {
        true => Ok(()),
        false => Err(Error::UrlStatusError(format!("{}: {}", url, resp.status()))),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_url_404() {
        assert!(check_url("https://nocduro.ca/invalid_page_name").is_err());
    }

    #[test]
    fn check_url_valid() {
        assert!(check_url("https://nocduro.ca/").is_ok());
        assert!(check_url("https://doc.rust-lang.org/").is_ok());
        assert!(check_url("https://doc.rust-lang.org").is_ok());
    }
}