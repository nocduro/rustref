extern crate url;
extern crate reqwest;

use self::url::Url;

pub struct API {
    api_key: String,
    api_email: String,
    api_user_service_key: Option<String>,
    organization_id: Option<String>, 
    base_url: Url,
    client: reqwest::Client,
    auth_type: AuthType,
}

pub enum AuthType {
    AuthKeyEmail,
    AuthUserService,
}

pub enum Error {
    InvalidOptions,
}

impl API {
    pub fn new(key: String, email: String, base_url: &str) -> Result<API, Error> {
        Ok(API {
            api_key: key,
            api_email: email,
            api_user_service_key: None,
            organization_id: None,
            base_url: Url::parse(base_url).map_err(|_| Error::InvalidOptions)?,
            client: reqwest::Client::new(),
            auth_type: AuthType::AuthKeyEmail,
        })
    }
}