use std::{
    convert::TryInto,
    fs::File,
    io::Read,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use atrium_api::{app::bsky, client::AtpServiceClient, com::atproto};
use bytes::Bytes;
use log::{error, info};
use once_cell::sync::Lazy;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use twitter_v2::{authorization::Oauth1aToken, TwitterApi};
use url::Url;

const TWEET_LENGTH: usize = 280;
const TOOT_LENGTH: usize = 500;
const BLUESKY_POST_LENGTH: usize = 300;
const MASTODON_FIXED_URL_LENGTH: usize = 23;
const SMALL_COMMERCIAL_AT: &str = "﹫";

#[derive(Deserialize)]
struct IntervalConfig {
    post_ttl: usize,
    fetch_interval: u64,
    post_interval: u64,
}

#[derive(Deserialize)]
struct RedisConfig {
    url: String,
}

#[derive(Deserialize, Clone)]
struct TwitterConfig {
    consumer_key: String,
    consumer_secret: String,
    token: String,
    secret: String,
}

#[derive(Deserialize, Clone)]
struct MastodonConfig {
    instance_url: Url,
    access_token: String,
}

#[derive(Deserialize, Clone)]
struct BlueskyConfig {
    host: String,
    identifier: String,
    password: String,
}

#[derive(Deserialize, Debug)]
struct DenylistConfig {
    names: Vec<String>,
    authors: Vec<String>,
    descriptions: Vec<String>,
}

impl DenylistConfig {
    fn contains(&self, repo: &Repo) -> bool {
        self.names.contains(&repo.name)
            || self.authors.contains(&repo.author)
            || self
                .descriptions
                .iter()
                .map(|description| {
                    repo.description
                        .to_lowercase()
                        .contains(&description.to_lowercase())
                })
                .any(|b| b)
    }
}

#[derive(Deserialize)]
struct Config {
    interval: IntervalConfig,
    redis: RedisConfig,
    #[serde(default)]
    twitter: Option<TwitterConfig>,
    #[serde(default)]
    mastodon: Option<MastodonConfig>,
    #[serde(default)]
    bluesky: Option<BlueskyConfig>,
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

async fn get_github_og_image(repo: &Repo) -> Result<Bytes> {
    static CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);

    let url = format!(
        "https://opengraph.githubassets.com/{}/{}/{}",
        random_string::generate(64, "0123456789abcdefghijklmnopqrstuvwxyz"),
        repo.author,
        repo.name
    );

    Ok(CLIENT
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?)
}

fn make_repo_title(repo: &Repo) -> String {
    if repo.author != repo.name {
        format!("{} / {}", repo.author, repo.name)
    } else {
        repo.name.clone()
    }
}

fn make_post_prefix(repo: &Repo) -> String {
    format!("{}: ", make_repo_title(repo))
}

fn make_post_stars(repo: &Repo) -> String {
    format!(" ★{}", repo.stars)
}

fn make_post_url(repo: &Repo) -> String {
    format!(" https://github.com/{}/{}", repo.author, repo.name)
}

fn repo_uri(repo: &Repo) -> String {
    format!("https://github.com/{}/{}", repo.author, repo.name)
}

fn make_post_description(repo: &Repo, length_left: usize) -> String {
    let description = repo.description.replace('@', SMALL_COMMERCIAL_AT);
    if repo.description.len() < length_left {
        description
    } else {
        format!("{} ...", description.split_at(length_left - 4).0)
    }
}

fn make_tweet(repo: &Repo) -> String {
    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);

    let length_left = TWEET_LENGTH - (prefix.len() + stars.len() + url.len());

    let description = make_post_description(repo, length_left);

    format!("{}{}{}{}", prefix, description, stars, url)
}

fn make_toot(repo: &Repo) -> String {
    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);

    let length_left = TOOT_LENGTH - (prefix.len() + stars.len() + MASTODON_FIXED_URL_LENGTH);

    let description = make_post_description(repo, length_left);

    format!("{}{}{}{}", prefix, description, stars, url)
}

async fn is_repo_posted(conn: &mut redis::aio::Connection, repo: &Repo) -> Result<bool> {
    Ok(conn
        .exists(format!("{}/{}", repo.author, repo.name))
        .await?)
}

async fn tweet(config: &TwitterConfig, content: String) -> Result<()> {
    let token = Oauth1aToken::new(
        &config.consumer_key,
        &config.consumer_secret,
        &config.token,
        &config.secret,
    );
    TwitterApi::new(token)
        .post_tweet()
        .text(content)
        .send()
        .await?;
    Ok(())
}

#[derive(Serialize, Debug)]
struct PostStatusesBody<'a> {
    status: &'a str,
    visibility: &'a str,
}

async fn toot(config: &MastodonConfig, content: &str) -> Result<()> {
    static CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);
    let url = config.instance_url.join("./api/v1/statuses")?;
    CLIENT
        .post(url)
        .bearer_auth(&config.access_token)
        .form(&PostStatusesBody {
            status: content,
            visibility: "unlisted",
        })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn post_bluesky(config: &BlueskyConfig, repo: &Repo) -> Result<()> {
    let thumbnail = get_github_og_image(repo).await?;

    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);

    let length_left = BLUESKY_POST_LENGTH - (prefix.len() + stars.len() + url.len());

    let description = make_post_description(repo, length_left);

    let text = format!("{}{}{}{}", prefix, description, stars, url);

    let client = AtpServiceClient::new(Arc::new(atrium_xrpc::client::reqwest::ReqwestClient::new(
        config.host.clone(),
    )));

    let session = client
        .com
        .atproto
        .server
        .create_session(atproto::server::create_session::Input {
            identifier: config.identifier.clone(),
            password: config.password.clone(),
        })
        .await?;
    let did = session.did.clone();

    let mut client = atrium_api::agent::AtpAgent::new(
        atrium_xrpc::client::reqwest::ReqwestClient::new(config.host.clone()),
    );
    client.set_session(session);

    let blob = client
        .api
        .com
        .atproto
        .repo
        .upload_blob(thumbnail.to_vec())
        .await?
        .blob;

    client
        .api
        .com
        .atproto
        .repo
        .create_record(atproto::repo::create_record::Input {
            collection: "app.bsky.feed.post".to_string(),
            record: atrium_api::records::Record::AppBskyFeedPost(Box::new(
                bsky::feed::post::Record {
                    created_at: OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)?,
                    embed: Some(bsky::feed::post::RecordEmbedEnum::AppBskyEmbedExternalMain(
                        Box::new(bsky::embed::external::Main {
                            external: bsky::embed::external::External {
                                description: repo.description.clone(),
                                thumb: Some(blob),
                                title: format!("{} / {}", repo.author, repo.name),
                                uri: repo_uri(repo),
                            },
                        }),
                    )),
                    entities: None,
                    facets: None,
                    langs: None,
                    reply: None,
                    text,
                },
            )),
            repo: did,
            rkey: None,
            swap_commit: None,
            validate: None,
        })
        .await?;

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
        if config.denylist.contains(&repo)
            || is_repo_posted(redis_conn, &repo)
                .await
                .context("While checking repo posted")?
        {
            continue;
        }

        if let Some(config) = &config.twitter {
            let content = make_tweet(&repo);
            if let Err(error) = tweet(config, content).await.context("While tweeting") {
                error!("{:#?}", error);
            }
        }

        if let Some(config) = &config.mastodon {
            let content = make_toot(&repo);
            if let Err(error) = toot(config, &content).await.context("While tooting") {
                error!("{:#?}", error);
            }
        }

        if let Some(config) = &config.bluesky {
            if let Err(error) = post_bluesky(config, &repo)
                .await
                .context("While posting to Bluesky")
            {
                error!("{:#?}", error);
            }
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
    use super::{make_tweet, parse_trending, DenylistConfig, Repo};

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
    fn test_denylistconfig_contains() {
        assert!(!DenylistConfig {
            authors: vec![],
            names: vec![],
            descriptions: vec![]
        }
        .contains(&repo!("foo", "bar", "somelongdescription", 0)));
        assert!(DenylistConfig {
            authors: vec!["foo".to_string()],
            names: vec![],
            descriptions: vec![]
        }
        .contains(&repo!("foo", "bar", "somelongdescription", 0)));
        assert!(!DenylistConfig {
            authors: vec!["bar".to_string()],
            names: vec![],
            descriptions: vec![]
        }
        .contains(&repo!("foo", "bar", "somelongdescription", 0)));
        assert!(DenylistConfig {
            authors: vec![],
            names: vec!["bar".to_string()],
            descriptions: vec![]
        }
        .contains(&repo!("foo", "bar", "somelongdescription", 0)));
        assert!(!DenylistConfig {
            authors: vec![],
            names: vec!["foo".to_string()],
            descriptions: vec![]
        }
        .contains(&repo!("foo", "bar", "somelongdescription", 0)));
        assert!(DenylistConfig {
            authors: vec![],
            names: vec![],
            descriptions: vec!["long".to_string()]
        }
        .contains(&repo!("foo", "bar", "somelongdescription", 0)));
        assert!(!DenylistConfig {
            authors: vec![],
            names: vec![],
            descriptions: vec!["foo".to_string()]
        }
        .contains(&repo!("foo", "bar", "somelongdescription", 0)));
        assert!(DenylistConfig {
            authors: vec![],
            names: vec![],
            descriptions: vec!["Long".to_string()]
        }
        .contains(&repo!("foo", "bar", "someloNgdescription", 0)));
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
