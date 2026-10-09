# Fixtures

Every fixture is listed here with where it came from. The kinds are defined in [`docs/design.md`](../../../docs/design.md):

- **recorded**: a real exchange captured by someone else, with source repository, commit, path and license. Only what a test needs is kept.
- **derived**: written from official documentation or a recorded fixture, with the source named.
- **synthetic**: invented, including everything the fake server generates.

No fixture comes from the owner's account and no test calls Last.fm. Credentials in fixtures are obvious sentinels.

| Path | Kind | Source | Notes |
|---|---|---|---|
| `official/method-parameters.json` | derived | Official method pages at https://www.last.fm/api, snapshot dated 2026-10-05 | The 57 method names with each documented parameter name and requirement label in page order, including `api_key`, `api_sig` and `sk`. Page prose is not kept. Where the page spells a name with a value set or batch index (`autocorrect[0\|1]`, `artist[i]`), `name` is the documented spelling and `normalized` the bare name. Read by `tests/inventory.rs`. |
| `recorded/lastfm-0.10.0-recent-tracks-extended-trimmed.json` | recorded, trimmed | Crate `lastfm` 0.10.0 (https://static.crates.io/crates/lastfm/lastfm-0.10.0.crate, sha256 `6e1c7d19b3dabcb2ccef2ffe389604248cdcbc063e3455e9079992e4ec336158`; repository https://github.com/lmammino/lastfm, commit `35e658b4a409195ab1b64f79f8047a797bbc908b`), `src/fixtures/recent_tracks_page.json` | MIT, copyright 2023 Luciano Mammino; the licence text is kept next to the fixture as `recorded/lastfm-0.10.0-LICENSE`. An extended `user.getRecentTracks` page of user `loige` at `perPage` 200. **Trimmed to the first four rows of 201** (the now-playing row and three scrobbles, three of the four with empty `mbid`s); the rows and `@attr` are verbatim, re-serialised compactly. Because the rows are cut, `@attr` (200 rows, total 290860) no longer matches them: the file is not a consistent page and is used only for row-shape decoding, and a test checks that a scan refuses it. Another person's public listening history, kept to the minimum needed. |
