# rust-trending

[![docker automated build](https://img.shields.io/docker/build/pbzweihander/rust-trending.svg)](https://hub.docker.com/r/pbzweihander/rust-trending/)
[![license: MIT](https://badgen.net/badge/license/MIT/green)](LICENSE)
[![@RustTrending](https://badgen.net/badge//twitter?icon=twitter)](https://twitter.com/RustTrending)

<img src="logo.svg" alt="Thinking With Rust" width="300px">

A twitter bot ([@RustTrending](https://twitter.com/RustTrending)) to tweet [trending rust repositories](https://github.com/trending/rust), inspired by [@TrendingGithub](https://twitter.com/TrendingGithub) and [@pythontrending](https://twitter.com/pythontrending).

## Usage

### Local

```bash
cargo build --release
cargo install
rust-trending config.toml
```

### Docker

```bash
docker run --rm -v $PWD/config.toml:/app/config.toml -d pbzweihander/rust-trending:latest
```

### Docker Compose

```bash
cp config.toml /srv/rust-trending/config.toml
docker-compose up -d
```

## License

This project is licensed under the terms of [MIT license](LICENSE).
