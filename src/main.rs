use {
    anyhow::{Context, Result as Fallible},
    log::{error, info},
    redis::AsyncCommands,
    serde::Deserialize,
    std::{
        fs::File,
        io::Read,
        time::{SystemTime, UNIX_EPOCH},
    },
};

const TWEET_LENGTH: usize = 280;

#[derive(Deserialize, Debug)]
struct IntervalConfig {
    tweet_ttl: usize,
    fetch_interval: u64,
    tweet_interval: u64,
}

#[derive(Deserialize, Debug)]
struct RedisConfig {
    url: String,
}

#[derive(Deserialize, Debug, Clone)]
struct TwitterConfig {
    disabled: bool,
    consumer_key: String,
    consumer_secret: String,
    access_key: String,
    access_secret: String,
}

#[derive(Deserialize, Debug)]
struct BlacklistConfig {
    names: Vec<String>,
    authors: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct Config {
    interval: IntervalConfig,
    redis: RedisConfig,
    twitter: TwitterConfig,
    blacklist: BlacklistConfig,
}

#[derive(Deserialize, Debug)]
struct Repo {
    author: String,
    description: String,
    name: String,
    stars: usize,
    url: String,
}

#[inline]
fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn read_config(path: &str) -> Fallible<Config> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(toml::from_str(&content)?)
}

async fn fetch_repos() -> Fallible<Vec<Repo>> {
    let resp =
        reqwest::get("https://github-trending-api.now.sh/repositories?language=rust&since=daily")
            .await?;
    Ok(resp.json().await?)
}

fn make_tweet(repo: &Repo) -> String {
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

    format!("{}{}{}{}", name, description, stars, url)
}

async fn is_repo_tweeted(conn: &mut redis::aio::Connection, repo: &Repo) -> Fallible<bool> {
    Ok(conn
        .exists(format!("{}/{}", repo.author, repo.name))
        .await?)
}

async fn tweet(config: TwitterConfig, content: String) -> Fallible<()> {
    let consumer = egg_mode::KeyPair::new(config.consumer_key, config.consumer_secret);
    let access = egg_mode::KeyPair::new(config.access_key, config.access_secret);
    let token = egg_mode::Token::Access { consumer, access };
    let tweet = egg_mode::tweet::DraftTweet::new(content);
    tweet.send(&token).await?;
    Ok(())
}

async fn mark_tweeted_repo(
    conn: &mut redis::aio::Connection,
    repo: &Repo,
    ttl: usize,
) -> Fallible<()> {
    conn.set_ex(format!("{}/{}", repo.author, repo.name), now_ts(), ttl)
        .await?;
    Ok(())
}

async fn main_loop(config: &Config, redis_conn: &mut redis::aio::Connection) -> Fallible<()> {
    let repos = fetch_repos().await.context("While fetching repo")?;

    for repo in repos {
        if config.blacklist.authors.contains(&repo.author)
            || config.blacklist.names.contains(&repo.name)
            || is_repo_tweeted(redis_conn, &repo)
                .await
                .context("While checking repo tweeted")?
        {
            continue;
        }

        if !config.twitter.disabled {
            let content = make_tweet(&repo);
            tweet(config.twitter.clone(), content)
                .await
                .context("While tweeting")?;
        }
        mark_tweeted_repo(redis_conn, &repo, config.interval.tweet_ttl)
            .await
            .context("While marking repo tweeted")?;

        info!("tweeted {} - {}", repo.author, repo.name);

        tokio::time::delay_for(tokio::time::Duration::from_secs(
            config.interval.tweet_interval,
        ))
        .await;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Fallible<()> {
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

        tokio::time::delay_for(tokio::time::Duration::from_secs(
            config.interval.fetch_interval,
        ))
        .await;
    }
}
