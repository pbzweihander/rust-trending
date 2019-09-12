use {
    failure::{Fallible, ResultExt},
    log::{error, info},
    serde::Deserialize,
    std::{
        fs::File,
        io::Read,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    },
};

const TWEET_LENGTH: usize = 280;

#[derive(Deserialize, Debug)]
struct IntervalConfig {
    tweet_ttl: u64,
    fetch_interval: u64,
    tweet_interval: u64,
}

#[derive(Deserialize, Debug)]
struct RedisConfig {
    url: String,
}

#[derive(Deserialize, Debug)]
struct TwitterTokenConfig {
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
    twitter: TwitterTokenConfig,
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

fn fetch_repos() -> Fallible<Vec<Repo>> {
    let mut resp =
        reqwest::get("https://github-trending-api.now.sh/repositories?language=rust&since=daily")?;
    Ok(resp.json()?)
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

fn is_repo_tweeted(conn: &mut redis::Connection, repo: &Repo) -> Fallible<bool> {
    let exists = redis::cmd("EXISTS")
        .arg(format!("{}/{}", repo.author, repo.name))
        .query(conn)?;

    Ok(exists)
}

fn tweet(config: &TwitterTokenConfig, content: &str) -> Fallible<()> {
    let consumer = oauth_client::Token::new(&config.consumer_key, &config.consumer_secret);
    let access = oauth_client::Token::new(&config.access_key, &config.access_secret);
    twitter_api::update_status(&consumer, &access, content)
}

fn mark_tweeted_repo(conn: &mut redis::Connection, repo: &Repo, ttl: u64) -> Fallible<()> {
    redis::cmd("SETEX")
        .arg(format!("{}/{}", repo.author, repo.name))
        .arg(ttl)
        .arg(now_ts())
        .query(conn)?;

    Ok(())
}

fn main_loop(config: &Config, redis_conn: &mut redis::Connection) -> Fallible<()> {
    let repos = fetch_repos().context("While fetching repo")?;

    for repo in repos {
        if config.blacklist.authors.contains(&repo.author)
            || config.blacklist.names.contains(&repo.name)
            || is_repo_tweeted(redis_conn, &repo).context("While checking repo tweeted")?
        {
            continue;
        }

        let content = make_tweet(&repo);
        tweet(&config.twitter, &content).context("While tweeting")?;
        mark_tweeted_repo(redis_conn, &repo, config.interval.tweet_ttl)
            .context("While marking repo tweeted")?;

        info!("tweeted {} - {}", repo.author, repo.name);

        thread::sleep(Duration::from_secs(config.interval.tweet_interval));
    }

    Ok(())
}

fn main() -> Fallible<()> {
    openssl_probe::init_ssl_cert_env_vars();
    env_logger::try_init().context("While initializing env_logger")?;

    let mut args = std::env::args();
    args.next();
    let config_file_path = args.next().unwrap_or_else(|| "./config.toml".to_string());
    let config = read_config(&config_file_path).context("While reading config file")?;

    let redis_client =
        redis::Client::open(config.redis.url.as_str()).context("While creating redis client")?;
    let mut redis_conn = redis_client
        .get_connection()
        .context("While connecting redis")?;

    loop {
        let res = main_loop(&config, &mut redis_conn);
        if let Err(e) = res {
            error!("{}", e);
        }

        thread::sleep(Duration::from_secs(config.interval.fetch_interval));
    }
}
