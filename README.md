# rust-trending

<img src="logo.svg" alt="RustTrending" width="300px">

A Fediverse and Bluesky bot to post [trending rust repositories](https://github.com/trending/rust), inspired by [@TrendingGithub] and [@pythontrending].

Check out in [Fediverse] and [Bluesky]!

## Usage

### Requirements

- Redis

### Local

```bash
RUST_LOG=info cargo run -- config.toml
```

### Docker

```bash
docker run -p 6379:6379 --rm -d redis
docker run --rm -v $PWD/config.toml:/config.toml -d ghcr.io/pbzweihander/rust-trending:latest
```

### Docker Compose

```bash
cp config.toml /srv/rust-trending/config.toml
docker-compose up -d
```

## License

This project is licensed under the terms of [MIT license][License].

[License]: LICENSE
[@TrendingGithub]: https://twitter.com/TrendingGithub
[@pythontrending]: https://twitter.com/pythontrending
[Fediverse]: https://yuri.garden/@RustTrending
[Bluesky]: https://bsky.app/profile/rusttrending.bsky.social
