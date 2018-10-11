extern crate config as configc;
extern crate url;
extern crate url_serde;

use Error;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Config {
    #[serde(with = "url_serde")]
    pub redis_url: url::Url,
    pub tweet_ttl: usize,
    pub fetch_interval: usize,
    pub tweet_interval: usize,
    pub twitter_token: TwitterToken,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TwitterToken {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub access_key: String,
    pub access_secret: String,
}

impl Config {
    pub fn from_file(filename: &str) -> Result<Self, Error> {
        let mut settings = configc::Config::default();
        settings.merge(configc::File::with_name(filename))?;
        Ok(settings.try_into()?)
    }
}
