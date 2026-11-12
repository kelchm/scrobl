# scrobl-cli

Keeps a local copy of a Last.fm listening history in a SQLite file. Unofficial.

Early development. Not affiliated with or endorsed by Last.fm.

It installs a program named `scrobl`. The program only reads from Last.fm, so it cannot change the account, and an API key is all it needs for a public history. It is built on the [`scrobl`](https://crates.io/crates/scrobl) crate.

## Usage

```sh
cargo install scrobl-cli
export SCROBL_API_KEY=your-api-key
scrobl sync --db history.db --user rj    # the first time
scrobl sync --db history.db              # every time after
```

- The first sync reads the whole history at one request a second: about a minute for every 10,000 scrobbles. It is safe to stop it and run it again.
- Every later sync reads again from fourteen days before the newest scrobble in the file, because scrobbles arrive late. It also reads any older period where Last.fm's count differs from the file's.
- `--full` reads everything again. That finds what no count shows, such as an old scrobble that was edited.
- A sync that fails says why in one line, exits with status 1 and keeps what it had stored.

## The file

| Table | Holds |
|---|---|
| `scrobbles` | One row per scrobble: `timestamp` in Unix seconds, `artist`, `track`, `album`, and the MusicBrainz ids Last.fm sent in `artist_mbid`, `track_mbid` and `album_mbid` |
| `removed` | Scrobbles that Last.fm no longer lists because they were deleted or edited there, each with `removed_at` |
| `loved` | The user's loved tracks as of the last sync, each with `loved_at` |
| `meta` | The user, and when the history and the loved tracks were last read |

Anything that reads SQLite can query it:

```sh
# The ten artists played most
sqlite3 history.db "SELECT artist, count(*) FROM scrobbles GROUP BY artist ORDER BY 2 DESC LIMIT 10"

# The last twenty plays, newest first
sqlite3 history.db "SELECT datetime(timestamp, 'unixepoch'), artist, track FROM scrobbles ORDER BY timestamp DESC, id DESC LIMIT 20"
```

Two plays in the same second are two rows, and `ORDER BY timestamp, id` is the order of listening. The addresses of cover images and track pages are not kept.

## The container image

Each [release](https://github.com/kelchm/scrobl/releases) is also a `linux/amd64` image, tagged with its version:

```sh
docker run --rm --read-only -e SCROBL_API_KEY -v scrobl:/data \
  ghcr.io/kelchm/scrobl:0.2.0 sync --db /data/history.db --user rj
```

The image runs as user and group 1000 and writes only under `/data`. Mount a volume there that this user can write to.

## Last.fm's terms

Using the API is subject to Last.fm's [API Terms of Service](https://www.last.fm/api/tos). You need your own API key, which you can [create here](https://www.last.fm/api/account/create). The MIT licence covers this program, not the data the API returns.

## License

[MIT](https://github.com/kelchm/scrobl/blob/main/LICENSE).
