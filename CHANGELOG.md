# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.3.0 - 2019-09-12

- Now with 2018 edition!
- Refactored code
- Remove asynchronous codes for to reduce code complexity
- Revised `config.toml` structure
- Updated dependencies

## 0.2.1 - 2018-10-14

- Timed tweeting with TimedStream struct instead of Interval
- Remove unused dependencies
  - futures-timer
- Updated dependencies
  - combine v3.6.0 -> v3.6.1
  - syn v0.15.9 -> v0.15.11

## 0.2.0 - 2018-10-13

- New feature: Blacklisting repository name or author
- Capitalized error contexts
- Config struct no longer implements Serialize trait
- Updated dependencies
  - futures v0.1.24 -> v0.1.25

## 0.1.1 - 2018-10-12

- Added whitespace after repository name notation when repo name and author are same.

## 0.1.0 - 2018-10-12

- first release
