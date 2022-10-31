# rust-trending

[![MIT License Badge]][License]
[![@RustTrending Badge]][@RustTrending]

<img src="logo.svg" alt="RustTrending" width="300px">

A Twitter and Mastodon bot to post [trending rust repositories](https://github.com/trending/rust), inspired by [@TrendingGithub] and [@pythontrending].

Check out in [Twitter](https://twitter.com/RustTrending) and [Mastodon](https://mastodon.pbzweihander.dev/@RustTrending)!

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

[MIT License Badge]: https://badgen.net/badge/license/MIT/green
[License]: LICENSE
[@RustTrending Badge]: https://badgen.net/twitter/follow/RustTrending
[@TrendingGithub]: https://twitter.com/TrendingGithub
[@pythontrending]: https://twitter.com/pythontrending
