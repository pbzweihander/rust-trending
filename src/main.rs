use std::{
    convert::TryInto,
    fs::File,
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use log::{error, info};
use redis::AsyncCommands;
use serde::Deserialize;

const TWEET_LENGTH: usize = 280;
const SMALL_COMMERCIAL_AT: &str = "﹫";

#[derive(Deserialize, Debug)]
struct IntervalConfig {
    post_ttl: usize,
    fetch_interval: u64,
    post_interval: u64,
}

#[derive(Deserialize, Debug)]
struct RedisConfig {
    url: String,
}

#[derive(Deserialize, Debug, Clone)]
struct TwitterConfig {
    consumer_key: String,
    consumer_secret: String,
    access_key: String,
    access_secret: String,
}

#[derive(Deserialize, Debug)]
struct DenylistConfig {
    names: Vec<String>,
    authors: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct Config {
    interval: IntervalConfig,
    redis: RedisConfig,
    #[serde(default)]
    twitter: Option<TwitterConfig>,
    denylist: DenylistConfig,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, Eq))]
struct Repo {
    author: String,
    description: String,
    name: String,
    stars: usize,
}

#[inline]
fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn read_config(path: &str) -> Result<Config> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(toml::from_str(&content)?)
}

fn parse_trending(html: String) -> Result<Vec<Repo>> {
    // Reference: https://github.com/huchenme/github-trending-api/blob/cf898c27850be407fb3f8dd31a4d1c3256ec6e12/src/functions/utils/fetch.js#L30-L103

    let html = scraper::Html::parse_document(&html);
    let repos = html
        .select(&".Box article.Box-row".try_into().unwrap())
        .filter_map(|repo| {
            let title = repo
                .select(&".h3".try_into().unwrap())
                .next()?
                .text()
                .fold(String::new(), |acc, s| acc + s);
            let mut title_split = title.split('/');

            let author = title_split.next()?.trim().to_string();
            let name = title_split.next()?.trim().to_string();

            let description = repo
                .select(&"p.my-1".try_into().unwrap())
                .next()
                .map(|e| {
                    e.text()
                        .fold(String::new(), |acc, s| acc + s)
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();

            let stars_text = repo
                .select(&".mr-3 svg[aria-label='star']".try_into().unwrap())
                .next()
                .and_then(|e| e.parent())
                .and_then(scraper::ElementRef::wrap)
                .map(|e| {
                    e.text()
                        .fold(String::new(), |acc, s| acc + s)
                        .trim()
                        .replace(',', "")
                })
                .unwrap_or_default();
            let stars = stars_text.parse().unwrap_or(0);

            Some(Repo {
                author,
                description,
                name,
                stars,
            })
        })
        .collect();

    Ok(repos)
}

async fn fetch_repos() -> Result<Vec<Repo>> {
    let resp = reqwest::get("https://github.com/trending/rust?since=daily")
        .await?
        .text()
        .await?;
    parse_trending(resp)
}

fn make_tweet(repo: &Repo) -> String {
    let name = if repo.author != repo.name {
        format!("{} / {}: ", repo.author, repo.name)
    } else {
        format!("{}: ", repo.name)
    };
    let stars = format!(" ★{}", repo.stars);
    let url = format!(" https://github.com/{}/{}", repo.author, repo.name);

    let length_left = TWEET_LENGTH - (name.len() + stars.len() + url.len());

    let description = repo.description.replace('@', SMALL_COMMERCIAL_AT);
    let description = if repo.description.len() < length_left {
        description
    } else {
        format!("{} ...", description.split_at(length_left - 4).0)
    };

    format!("{}{}{}{}", name, description, stars, url)
}

async fn is_repo_posted(conn: &mut redis::aio::Connection, repo: &Repo) -> Result<bool> {
    Ok(conn
        .exists(format!("{}/{}", repo.author, repo.name))
        .await?)
}

async fn tweet(config: TwitterConfig, content: String) -> Result<()> {
    let consumer = egg_mode::KeyPair::new(config.consumer_key, config.consumer_secret);
    let access = egg_mode::KeyPair::new(config.access_key, config.access_secret);
    let token = egg_mode::Token::Access { consumer, access };
    let tweet = egg_mode::tweet::DraftTweet::new(content);
    tweet.send(&token).await?;
    Ok(())
}

async fn mark_posted_repo(
    conn: &mut redis::aio::Connection,
    repo: &Repo,
    ttl: usize,
) -> Result<()> {
    conn.set_ex(format!("{}/{}", repo.author, repo.name), now_ts(), ttl)
        .await?;
    Ok(())
}

async fn main_loop(config: &Config, redis_conn: &mut redis::aio::Connection) -> Result<()> {
    let repos = fetch_repos().await.context("While fetching repo")?;

    for repo in repos {
        if config.denylist.authors.contains(&repo.author)
            || config.denylist.names.contains(&repo.name)
            || is_repo_posted(redis_conn, &repo)
                .await
                .context("While checking repo posted")?
        {
            continue;
        }

        if let Some(config) = &config.twitter {
            let content = make_tweet(&repo);
            tweet(config.clone(), content)
                .await
                .context("While tweeting")?;
        }
        mark_posted_repo(redis_conn, &repo, config.interval.post_ttl)
            .await
            .context("While marking repo posted")?;

        info!("posted {} - {}", repo.author, repo.name);

        tokio::time::sleep(tokio::time::Duration::from_secs(
            config.interval.post_interval,
        ))
        .await;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::try_init().context("While initializing env_logger")?;

    let mut args = std::env::args();
    args.next();
    let config_file_path = args.next().unwrap_or_else(|| "./config.toml".to_string());
    let config = read_config(&config_file_path).context("While reading config file")?;

    let redis_client =
        redis::Client::open(config.redis.url.as_str()).context("While creating redis client")?;
    let mut redis_conn = redis_client
        .get_async_connection()
        .await
        .context("While connecting redis")?;

    loop {
        let res = main_loop(&config, &mut redis_conn).await;
        if let Err(e) = res {
            error!("{:#}", e);
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(
            config.interval.fetch_interval,
        ))
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::{make_tweet, parse_trending, Repo};

    const TEST_HTML: &str = include_str!("../testdata/test.html");

    macro_rules! repo {
        ( $author:expr, $name:expr, $description:expr, $stars:expr ) => {
            Repo {
                author: $author.to_string(),
                name: $name.to_string(),
                description: $description.to_string(),
                stars: $stars,
            }
        };
    }

    #[test]
    fn test_parse_trending() {
        let repos = parse_trending(TEST_HTML.to_string()).unwrap();
        assert_eq!(
            repos[..5].to_vec(),
            vec![
                repo!("servo", "servo", "The Servo Browser Engine", 18622),
                repo!(
                    "timberio",
                    "vector",
                    "A high-performance, end-to-end observability data platform.",
                    5672
                ),
                repo!(
                    "rust-lang",
                    "rust",
                    "Empowering everyone to build reliable and efficient software.",
                    49626
                ),
                repo!(
                    "wasmerio",
                    "wasmer",
                    "🚀 The leading WebAssembly Runtime supporting WASI and Emscripten",
                    6806
                ),
                repo!(
                    "firecracker-microvm",
                    "firecracker",
                    "Secure and fast microVMs for serverless computing.",
                    13092
                ),
            ]
        );
    }

    #[test]
    fn test_make_tweet() {
        assert_eq!(
            make_tweet(&repo!(
                "wez",
                "wezterm",
                "A GPU-accelerated cross-platform terminal emulator and multiplexer written by @wez and implemented in Rust",
                5924
            )),
            "wez / wezterm: A GPU-accelerated cross-platform terminal emulator and multiplexer written by ﹫wez and implemented in Rust ★5924 https://github.com/wez/wezterm"
        );
        assert_eq!(
            make_tweet(&repo!(
                "AlfioEmanueleFresta",
                "xdg-credentials-portal",
                "FIDO2 (WebAuthn) and FIDO U2F platform library for Linux written in Rust; includes a proposal for a new D-Bus Portal interface for FIDO2, accessible from Flatpak apps and Snaps key",
                192
            )),
            "AlfioEmanueleFresta / xdg-credentials-portal: FIDO2 (WebAuthn) and FIDO U2F platform library for Linux written in Rust; includes a proposal for a new D-Bus Portal interface for FIDO2, accessible from Flatpak ... ★192 https://github.com/AlfioEmanueleFresta/xdg-credentials-portal"
        );
        assert_eq!(
            make_tweet(&repo!(
                "meilisearch",
                "meilisearch",
                "A lightning-fast search engine that fits effortlessly into your apps, websites, and workflow.",
                30388
            )),
            "meilisearch: A lightning-fast search engine that fits effortlessly into your apps, websites, and workflow. ★30388 https://github.com/meilisearch/meilisearch"
        );
    }
}
