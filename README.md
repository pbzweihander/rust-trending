# rust-trending

[![Docker Hub Automated Build Badge]][Docker Hub]
[![MIT License Badge]][License]
[![Twitter @RustTrending Badge]][Twitter @RustTrending]

<img src="logo.svg" alt="Thinking With Rust" width="300px">

A twitter bot ([@RustTrending][Twitter @RustTrending]) to tweet
[trending rust repositories](https://github.com/trending/rust), inspired by
[@TrendingGithub](https://twitter.com/TrendingGithub) and
[@pythontrending](https://twitter.com/pythontrending).

## Usage

### Requirements

- Redis

### Local

```bash
cargo build --release
cargo install
rust-trending config.toml
```

### Docker

```bash
docker run -p 6379:6379 --rm -d redis
docker run --rm -v $PWD/config.toml:/app/config.toml -d pbzweihander/rust-trending:latest
```

### Docker Compose

```bash
cp config.toml /srv/rust-trending/config.toml
docker-compose up -d
```

## License

This project is licensed under the terms of [MIT license][License].

[Docker Hub Automated Build Badge]: https://img.shields.io/docker/build/pbzweihander/rust-trending.svg
[Docker Hub]: https://hub.docker.com/r/pbzweihander/rust-trending/
[MIT License Badge]: https://badgen.net/badge/license/MIT/green
[License]: LICENSE
[Twitter @RustTrending Badge]: https://badgen.net/twitter/follow/RustTrending
[Twitter @RustTrending]: https://twitter.com/RustTrending
