use std::{
    convert::TryInto,
    fs::File,
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use atrium_api::{app::bsky, com::atproto, types::TryIntoUnknown};
use bytes::Bytes;
use log::{error, info};
use once_cell::sync::Lazy;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use url::Url;

const MASTODON_POST_LENGTH: usize = 500;
const MISSKEY_POST_LENGTH: usize = 3000;
const BLUESKY_POST_LENGTH: usize = 300;
const MASTODON_FIXED_URL_LENGTH: usize = 23;
const SMALL_COMMERCIAL_AT: &str = "﹫";

#[derive(Deserialize)]
struct IntervalConfig {
    post_ttl: u64,
    fetch_interval: u64,
    post_interval: u64,
}

#[derive(Deserialize)]
struct RedisConfig {
    url: String,
}

#[derive(Deserialize, Clone)]
struct MastodonConfig {
    instance_url: Url,
    access_token: String,
}

#[derive(Deserialize, Clone)]
struct MisskeyConfig {
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
            || self.descriptions.iter().any(|description| {
                repo.description
                    .to_lowercase()
                    .contains(&description.to_lowercase())
            })
    }
}

#[derive(Deserialize)]
struct Config {
    interval: IntervalConfig,
    redis: RedisConfig,
    #[serde(default)]
    mastodon: Option<MastodonConfig>,
    #[serde(default)]
    misskey: Option<MisskeyConfig>,
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
    if repo.description.graphemes(true).count() < length_left {
        description
    } else {
        format!(
            "{} ...",
            description
                .graphemes(true)
                .take(length_left - 4)
                .collect::<String>()
        )
    }
}

fn make_mastodon_post(repo: &Repo) -> String {
    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);

    let length_left =
        MASTODON_POST_LENGTH - (prefix.len() + stars.len() + MASTODON_FIXED_URL_LENGTH);

    let description = make_post_description(repo, length_left);

    format!("{}{}{}{}", prefix, description, stars, url)
}

fn make_misskey_post(repo: &Repo) -> String {
    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);

    let length_left = MISSKEY_POST_LENGTH - (prefix.len() + stars.len() + url.len());

    let description = make_post_description(repo, length_left);

    format!("{}{}{}{}", prefix, description, stars, url)
}

async fn is_repo_posted(conn: &mut redis::aio::MultiplexedConnection, repo: &Repo) -> Result<bool> {
    Ok(conn
        .exists(format!("{}/{}", repo.author, repo.name))
        .await?)
}

#[derive(Serialize, Debug)]
struct MastodonPostStatusesBody<'a> {
    status: &'a str,
    visibility: &'a str,
}

async fn post_mastodon(config: &MastodonConfig, content: &str) -> Result<()> {
    static CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);
    let url = config.instance_url.join("./api/v1/statuses")?;
    CLIENT
        .post(url)
        .bearer_auth(&config.access_token)
        .form(&MastodonPostStatusesBody {
            status: content,
            visibility: "unlisted",
        })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

#[derive(Serialize, Debug)]
struct MisskeyCreateNoteBody<'a> {
    text: &'a str,
    visibility: &'a str,
}

async fn post_misskey(config: &MisskeyConfig, content: &str) -> Result<()> {
    static CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);
    let url = config.instance_url.join("./api/notes/create")?;
    CLIENT
        .post(url)
        .bearer_auth(&config.access_token)
        .json(&MisskeyCreateNoteBody {
            text: content,
            visibility: "home",
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

    let agent = atrium_api::agent::atp_agent::AtpAgent::new(
        atrium_xrpc_client::reqwest::ReqwestClient::new(config.host.clone()),
        atrium_api::agent::atp_agent::store::MemorySessionStore::default(),
    );
    agent
        .login(&config.identifier, &config.password)
        .await
        .context("failed to login to Bluesky")?;
    let did = agent.did().await.unwrap();

    let blob = agent
        .api
        .com
        .atproto
        .repo
        .upload_blob(thumbnail.to_vec())
        .await?;

    agent
        .api
        .com
        .atproto
        .repo
        .create_record(
            atproto::repo::create_record::InputData {
                collection: atrium_api::types::string::Nsid::new("app.bsky.feed.post".to_string())
                    .unwrap(),
                record: bsky::feed::post::RecordData {
                    created_at: atrium_api::types::string::Datetime::now(),
                    embed: Some(atrium_api::types::Union::Refs(
                        bsky::feed::post::RecordEmbedRefs::AppBskyEmbedExternalMain(Box::new(
                            bsky::embed::external::MainData {
                                external: bsky::embed::external::ExternalData {
                                    description: repo.description.clone(),
                                    thumb: Some(blob.data.blob),
                                    title: format!("{} / {}", repo.author, repo.name),
                                    uri: repo_uri(repo),
                                }
                                .into(),
                            }
                            .into(),
                        )),
                    )),
                    entities: None,
                    facets: None,
                    labels: None,
                    langs: None,
                    reply: None,
                    tags: None,
                    text,
                }
                .try_into_unknown()
                .context("failed to convert record")?,
                repo: did.into(),
                rkey: None,
                swap_commit: None,
                validate: None,
            }
            .into(),
        )
        .await?;

    Ok(())
}

#[allow(dependency_on_unit_never_type_fallback)]
async fn mark_posted_repo(
    conn: &mut redis::aio::MultiplexedConnection,
    repo: &Repo,
    ttl: u64,
) -> Result<()> {
    conn.set_ex(format!("{}/{}", repo.author, repo.name), now_ts(), ttl)
        .await?;
    Ok(())
}

async fn main_loop(
    config: &Config,
    redis_conn: &mut redis::aio::MultiplexedConnection,
) -> Result<()> {
    let repos = fetch_repos().await.context("While fetching repo")?;

    for repo in repos {
        if config.denylist.contains(&repo)
            || is_repo_posted(redis_conn, &repo)
                .await
                .context("While checking repo posted")?
        {
            continue;
        }

        if let Some(config) = &config.mastodon {
            let content = make_mastodon_post(&repo);
            if let Err(error) = post_mastodon(config, &content)
                .await
                .context("While posting to Mastodon")
            {
                error!("{:#?}", error);
            }
        }

        if let Some(config) = &config.misskey {
            let content = make_misskey_post(&repo);
            if let Err(error) = post_misskey(config, &content)
                .await
                .context("While posting to Misskey")
            {
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
        .get_multiplexed_async_connection()
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
    use super::{parse_trending, DenylistConfig, Repo};

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
}
