# scrobl

A Rust library for the Last.fm API, and a command-line tool built on it that keeps a local copy of a listening history. Unofficial.

Early development. Not affiliated with or endorsed by Last.fm.

| | What it is | Use it to |
|---|---|---|
| [`scrobl`](crates/scrobl/README.md) | A Rust crate, [on crates.io](https://crates.io/crates/scrobl) | Call the Last.fm API from a program of your own |
| [`scrobl-cli`](crates/scrobl-cli/README.md) | A program named `scrobl`, [on crates.io](https://crates.io/crates/scrobl-cli) | Keep one user's listening history in a SQLite file |
| [`ghcr.io/kelchm/scrobl`](crates/scrobl-cli/README.md#the-container-image) | That program as a container image | Run it under Docker or in a cluster |

Each name links to its own page. The three are released together under one version.

- [Design](docs/design.md): what the library is and why
- [Endpoints](docs/endpoints.md): every Last.fm method and how far it is verified

## Last.fm's terms

Using the API is subject to Last.fm's [API Terms of Service](https://www.last.fm/api/tos). You need your own API key, which you can [create here](https://www.last.fm/api/account/create). The MIT licence covers this code, not the data the API returns.

## Development

Tool versions are pinned in `mise.toml`.

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all-features
cargo test --no-default-features -p scrobl
cargo llvm-cov --all-features --fail-under-lines 95
```

Tests never call Last.fm. They use fixtures and a local fake server.

How changes are made is in [`AGENTS.md`](AGENTS.md). The minimum Rust version is 1.88 for the library and 1.89 for the tool, and CI tests each on it.

## License

[MIT](LICENSE). No fixture is copied from another project. Where each came from is listed in [`crates/scrobl/fixtures/README.md`](crates/scrobl/fixtures/README.md).
