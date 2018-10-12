extern crate config as configc;
extern crate url;
extern crate url_serde;

use {Error, Repo};

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    #[serde(with = "url_serde")]
    pub redis_url: url::Url,
    pub tweet_ttl: usize,
    pub fetch_interval: usize,
    pub tweet_interval: usize,
    pub twitter_token: TwitterToken,
    pub blacklist: Blacklist,
}

#[derive(Clone, Deserialize, Debug)]
pub struct TwitterToken {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub access_key: String,
    pub access_secret: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct Blacklist {
    pub names: Vec<String>,
    pub authors: Vec<String>,
}

impl Config {
    pub fn from_file(filename: &str) -> Result<Self, Error> {
        let mut settings = configc::Config::default();
        settings.merge(configc::File::with_name(filename))?;
        Ok(settings.try_into()?)
    }
}

impl Blacklist {
    pub fn is_listed(&self, repo: &Repo) -> bool {
        self.authors.iter().any(|a| &repo.author == a) || self.names.iter().any(|n| &repo.name == n)
    }
}
