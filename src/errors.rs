use std::result;
use cloudflare;
use toml;
use reqwest;
use std;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Cloudflare(cloudflare::Error),
    Reqwest(reqwest::Error),
    Toml(toml::de::Error),
    Lock(String),
    Io(std::io::Error),
    RedirectError(RedirectError),
    RedirectErrors(Vec<RedirectError>),
}

#[derive(Debug)]
pub enum RedirectError {
    BadUrl(String),
    InvalidPage(String),
    DuplicateRule(String),
}

impl From<cloudflare::Error> for Error {
    fn from(err: cloudflare::Error) -> Error {
        Error::Cloudflare(err)
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Error {
        Error::Reqwest(err)
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Error {
        Error::Toml(err)
    }
}

impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(_err: std::sync::PoisonError<T>) -> Error {
        Error::Lock("ReadWrite lock was poisoned!".to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<RedirectError> for Error {
    fn from(err: RedirectError) -> Error {
        Error::RedirectError(err)
    }
}

impl From<Vec<RedirectError>> for Error {
    fn from(err: Vec<RedirectError>) -> Error {
        Error::RedirectErrors(err)
    }
}
