use rust_trending::*;

#[tokio::main]
async fn main() {
    let repos = fetch_repos().await.unwrap();
    for repo in repos {
        println!(
            "{}/{}\n\t★{}\n\t{}",
            repo.author, repo.name, repo.stars, repo.description
        );
    }
}
