extern crate url;
extern crate url_serde;

#[derive(Deserialize, Debug, Clone)]
pub struct Repo {
    pub author: String,
    pub description: String,
    pub forks: usize,
    pub name: String,
    pub stars: usize,
    #[serde(with = "url_serde")]
    pub url: url::Url,
}
