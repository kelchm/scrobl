# Working on scrobl

Rules for anyone changing this repository, human or agent. [`docs/design.md`](docs/design.md) says what the library is and why; this file says how changes are made.

## Hard limits

- Never call the live Last.fm service without the owner's explicit approval for that run. Tests use fixtures and the local fake server. `crates/scrobl/tests/live.rs` is ignored by default; never run it with `--ignored` on your own initiative, and never in CI.
- Never write to a Last.fm account from a test, an example or a script.
- Never commit a credential, a session key, or a captured response that the owner has not approved for publication. Captures go in `.scratch/`, which is ignored.
- Never publish to crates.io, change `publish = false`, tag a release or change repository settings. Those are the owner's decisions.
- Keep "Not affiliated with or endorsed by Last.fm" where it stands. Do not use Last.fm's logos or styling, and do not use "Audioscrobbler" or "Scrobbler" as names for anything here.
- Never push to `main`. Never merge a pull request; the owner merges.

## Pull requests

- One logical change per pull request, small enough to review in one sitting.
- Pull requests are squash-merged. The title becomes the commit on `main` and the description becomes its body, so write both for someone reading `git log` a year from now.
- The title follows Conventional Commits: `type: summary`, or `type!: summary` for a breaking change to the public API. The summary is imperative, lower case, without a full stop, and at most 72 characters in all.
- Types: `feat` (new behaviour a user can see), `fix` (a bug), `docs`, `test`, `refactor` (no behaviour change), `perf`, `build` (dependencies, packaging, toolchain), `ci`, `chore`. No scopes.
- The description says what changed and why, what was verified and how, and anything a reviewer should distrust. A breaking change says how to migrate.
- Stacked pull requests are fine. Land them bottom-up; after the one below is squashed, rebase the next onto `main` and force-push it. Force-pushing your own pull request branch is allowed; force-pushing anyone else's is not.
- A change to the public API, to signing, to history pagination or to write safety gets a review from a model or person other than its author before it is called ready.

## Commits

- Commits inside a pull request are squashed away, so they do not need the title format. They do need to be signed, and to say what they do in an imperative sentence.
- If signing fails, stop and ask the owner. Do not bypass it.
- An agent adds a `Co-Authored-By` trailer naming the model, and repeats it at the end of the pull request description so it survives the squash.

## Before opening a pull request

Tool versions are pinned in `mise.toml`. All of these must pass:

```sh
cargo fmt --all --check
RUSTFLAGS="-D warnings" cargo clippy --locked --all-targets --all-features
RUSTFLAGS="-D warnings" cargo clippy --locked --all-targets --no-default-features
cargo test --locked --all-features
cargo test --locked --no-default-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
cargo llvm-cov --locked --all-features --fail-under-lines 95
cargo deny check advisories licenses
```

Report what you ran and what it printed. Do not say a check passes unless you ran it.

## Code

- No `unsafe`. No `unwrap`, `expect`, `panic`, indexing or printing in library code; the workspace lints enforce it. Tests may allow them at the top of the file.
- Nothing that formats an error, a request or a response may show a credential, a URL or text the network chose. A new type that holds one gets a hand-written `Debug`, and a test that proves it.
- A write is never retried and never sent twice. Anything that could change that needs a test that counts requests at the fake server.
- A bug fix comes with a test that fails without the fix.
- A new public item has documentation and a test. Line coverage does not go below 95%; raise the floor in CI when coverage rises.
- A new dependency needs a reason in the pull request. Prefer none. Its licence must be on the list in `deny.toml`.
- The minimum Rust version is in `Cargo.toml` and is tested in CI. Raising it is its own pull request, typed `build!`.
- Match the code around you: its naming, its comment density, its test style. Comments say why, not what.

## Documents and fixtures

- Markdown prose is not hard-wrapped: one paragraph or list item per line.
- Plain, short sentences. Say what is true today; do not describe what is planned as if it existed.
- The table in `docs/endpoints.md` is generated. Change `crates/scrobl/src/protocol/methods.rs` and run `SCROBL_BLESS=1 cargo test --test inventory`. A method's status there is a claim: raise it only with the evidence its level requires.
- Every fixture is listed in `crates/scrobl/fixtures/README.md` as recorded, derived or synthetic, with where it came from. Recorded fixtures are the owner's own captures, kept byte for byte.
- When behaviour changes, change `README.md` and `docs/design.md` in the same pull request.
