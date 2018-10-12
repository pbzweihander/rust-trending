extern crate failure;
#[macro_use]
extern crate serde_derive;
extern crate chrono;
extern crate futures;
extern crate futures_timer;
extern crate hyper;
extern crate hyper_tls;
extern crate oauth_client;
extern crate serde_json;
extern crate tokio;
extern crate twitter_api;

pub use failure::Error;

pub mod config;
mod repo;
mod storage;

pub use config::Config;
use repo::Repo;
use storage::Storage;

use chrono::{DateTime, Utc};
use futures::Future;
use hyper::{Body, Client};
use hyper_tls::HttpsConnector;
use oauth_client::Token;

const TWEET_LENGTH: usize = 280;

fn err_log(e: &Error) {
    use chrono::Local;
    eprintln!("At {}", Local::now());
    eprintln!("Error: {}", e);
    eprintln!("Error chain:");
    for c in e.iter_chain() {
        eprintln!("- {}", c);
    }
}

fn fetch_repos() -> impl Future<Item = Vec<Repo>, Error = Error> {
    use futures::future::result;
    use futures::Stream;
    use hyper::Request;

    let con = HttpsConnector::new(4).expect("TLS initialization failed");
    let client = Client::builder().build(con);

    let req =
        Request::get("https://github-trending-api.now.sh/repositories?language=rust&since=daily")
            .body(Body::empty())
            .unwrap();
    let resp = client.request(req);

    resp.and_then(|resp| resp.into_body().concat2())
        .map_err(Into::into)
        .and_then(|body| result(serde_json::from_slice(&body).map_err(Into::into)))
}

fn tweet_repo(
    consumer: &Token,
    access: &Token,
    repo: &Repo,
) -> impl Future<Item = DateTime<Utc>, Error = Error> {
    let name = if repo.author != repo.name {
        format!("{} / {}: ", repo.author, repo.name)
    } else {
        format!("{}: ", repo.name)
    };
    let stars = format!(" ★{}", repo.stars);
    let url = format!(" {}", repo.url);

    let length_left = TWEET_LENGTH - (name.len() + stars.len() + url.len());

    let description = if repo.description.len() < length_left {
        repo.description.to_string()
    } else {
        format!("{} ...", repo.description.split_at(length_left - 4).0)
    };

    let tweet = format!("{}{}{}{}", name, description, stars, url);
    twitter_api::update_status(consumer, access, &tweet)
        .map(|_| Utc::now())
        .map_err(|e| e.context("Tweet error").into())
}

pub struct RustTrending {
    config: Config,
    storage: Storage,
    token: (Token<'static>, Token<'static>),
}

impl RustTrending {
    pub fn new(config: Config) -> Result<Self, Error> {
        let storage = Storage::new(&config)?;

        let con_token = Token::new(
            config.twitter_token.consumer_key.clone(),
            config.twitter_token.consumer_secret.clone(),
        );
        let acc_token = Token::new(
            config.twitter_token.access_key.clone(),
            config.twitter_token.access_secret.clone(),
        );
        let token = (con_token, acc_token);

        Ok(RustTrending {
            config,
            storage,
            token,
        })
    }

    pub fn run_loop(self) -> impl Future<Item = (), Error = Error> {
        use futures::future::ok;
        use futures::stream::{iter_ok, Stream};
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        use tokio::timer::Interval;

        let fetch_interval = Duration::from_secs(self.config.fetch_interval as u64);
        let tweet_interval = Duration::from_secs(self.config.tweet_interval as u64);
        let storage = Arc::new(self.storage);
        let storage1 = storage.clone();
        let token = Arc::new(self.token);
        let blacklist = Arc::new(self.config.blacklist);

        let fetch_stream = Interval::new(Instant::now(), fetch_interval)
            .map(move |_| {
                let storage = storage.clone();
                let blacklist = blacklist.clone();
                fetch_repos()
                    .map(iter_ok)
                    .flatten_stream()
                    .and_then(move |r| storage.is_repo_already_tweeted(&r).map(|b| (r, b)))
                    .filter(|(_, is_repo_already_tweeted)| !is_repo_already_tweeted)
                    .map(|(r, _)| r)
                    .filter(move |r| {
                        let blacklist = blacklist.clone();
                        !blacklist.is_listed(&r)
                    })
            }).flatten()
            .map_err(|e| e.context("Fetch stream error").into());

        Interval::new(Instant::now(), tweet_interval)
            .map_err(Into::into)
            .zip(fetch_stream)
            .for_each(move |(_, r)| {
                let storage = storage1.clone();
                let token = token.clone();
                let r1 = r.clone();
                let r2 = r.clone();

                tweet_repo(&token.0, &token.1, &r)
                    .and_then(move |ts| storage.mark_repo_as_tweeted(&r1, ts).map(move |_| ts))
                    .map(move |ts| {
                        println!("{}, tweeted {} - {}", ts, r2.author, r2.name);
                    })
            })
            .map_err(Into::<Error>::into)
            .map_err(|e| e.context("Tweet stream error").into())
            .or_else(|e| {
                err_log(&e);
                ok(())
            })
    }
}
