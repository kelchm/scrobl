# Fixtures

Every fixture is listed here with where it came from. The kinds are defined in [`docs/design.md`](../../../docs/design.md):

- **recorded**: a real exchange captured by someone else, with source repository, commit, path and license. Only what a test needs is kept.
- **derived**: written from official documentation or a recorded fixture, with the source named.
- **synthetic**: invented, including everything the fake server generates.

No fixture comes from the owner's account and no test calls Last.fm. Credentials in fixtures are obvious sentinels.

| Path | Kind | Source | Notes |
|---|---|---|---|
| `official/method-parameters.json` | derived | Official method pages at https://www.last.fm/api, snapshot dated 2026-10-05 | The 57 method names with each documented parameter name and requirement label in page order, including `api_key`, `api_sig` and `sk`. Page prose is not kept. Where the page spells a name with a value set or batch index (`autocorrect[0\|1]`, `artist[i]`), `name` is the documented spelling and `normalized` the bare name. Read by `tests/inventory.rs`. |
