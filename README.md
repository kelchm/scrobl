# scrobl

A Rust client for the full Last.fm API: reads, authentication and scrobbling.

Early development. Not affiliated with or endorsed by Last.fm.

- [Design](docs/design.md)
- [Endpoints](docs/endpoints.md): every method and how far it is verified

## Development

Tool versions are pinned in `mise.toml`.

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all-features
cargo test --no-default-features
```

Tests never call Last.fm. They use fixtures and a local fake server.
