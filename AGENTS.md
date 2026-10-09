# Working on scrobl

[`docs/design.md`](docs/design.md) says what the library is and why. This file says how changes are made.

## Pull requests

- One logical change each. They are squash-merged: the title becomes the commit on `main`.
- Title: `type: summary`, or `type!:` for a breaking API change. Lower case, imperative, no full stop, 72 characters at most. Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`.
- Commits are signed. If signing fails, ask the owner; do not bypass it.
- A change to the public API, signing, history pagination or write safety is reviewed by someone other than its author.
- Run what CI runs (`.github/workflows/ci.yml`) before opening one, and report only checks you ran.

## Code

- No error, `Debug` or log output may show a credential, a URL or text the network chose.
- A write is never retried or sent twice.
- A bug fix comes with a test that fails without it. A new public item has documentation and a test.
- A new dependency needs a reason. Prefer none.
- The table in `docs/endpoints.md` is generated: `SCROBL_BLESS=1 cargo test --test inventory`.
- Every fixture is listed with its source in `crates/scrobl/fixtures/README.md`.
- Tests never call Last.fm. `tests/live.rs` runs only when the owner asks, and what it captures stays in `.scratch/`.
- The project is unofficial: keep the "not affiliated" line, and name nothing "Audioscrobbler" or "Scrobbler".
- Markdown prose is not hard-wrapped.
