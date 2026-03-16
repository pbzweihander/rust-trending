use std::{collections::HashSet, convert::TryInto};

use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
pub struct Repo {
    pub author: String,
    pub description: String,
    pub name: String,
    pub stars: usize,
}

pub fn parse_trending(html: String) -> anyhow::Result<Vec<Repo>> {
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
                .select(&"svg[aria-label='star']".try_into().unwrap())
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

pub async fn fetch_repos() -> anyhow::Result<Vec<Repo>> {
    let daily = reqwest::get("https://github.com/trending/rust?since=daily")
        .await?
        .text()
        .await?;
    let daily = parse_trending(daily)?;
    let weekly = reqwest::get("https://github.com/trending/rust?since=weekly")
        .await?
        .text()
        .await?;
    let weekly = parse_trending(weekly)?;
    let monthly = reqwest::get("https://github.com/trending/rust?since=monthly")
        .await?
        .text()
        .await?;
    let monthly = parse_trending(monthly)?;
    let mut repos = HashSet::new();
    repos.extend(daily);
    repos.extend(weekly);
    repos.extend(monthly);
    Ok(repos.into_iter().collect())
}
