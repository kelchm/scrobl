# Endpoints

Every method in the official Last.fm API index, with the verb, credentials, paging and parameters its documentation gives, and the level scrobl has reached for it. The levels (`inventoried`, `request-verified`, `typed-derived`, `fixture-verified`, `live-verified`) are defined under Coverage in [`design.md`](design.md), and this table is the release claim: a method is only as verified as its row says.

The rows come from the method table in `crates/scrobl/src/protocol/methods.rs` and are checked against a snapshot of the official method pages dated 2026-10-05 (`crates/scrobl/fixtures/official/method-parameters.json`). Run `SCROBL_BLESS=1 cargo test --test inventory` to regenerate the block below; do not edit it by hand.

Credentials: every method needs an API key. A signature is an `api_sig` made with the API secret. A session is a user's session key, sent as `sk`. In the Parameters column, required parameters are plain, `name\*` is required unless an alternative such as `mbid` is given, and _italic_ names are optional. `name[i]` takes a batch index (`artist[0]`, `artist[1]`, ...). `api_key`, `api_sig`, `sk`, `method`, `format` and `callback` are implied by the credentials and never listed.

Out of scope, because the official page marks it deprecated or because it is only another encoding of the same methods: the Radio API, the Playlists API, Submissions Protocol 1.2.1, XML-RPC and XML output. Website scraping and history editing are out of scope permanently.

<!-- BEGIN GENERATED -->

| Method | Verb | Credentials | Write | Paging | Parameters | Typed model | Status |
|---|---|---|---|---|---|---|---|
| **album** | | | | | | | |
| `album.addTags` | POST | API key, signature, session | yes | — | artist, album, tags | — | inventoried |
| `album.getInfo` | GET | API key | — | — | artist\*, album\*, _mbid_, _autocorrect_, _username_, _lang_ | — | inventoried |
| `album.getTags` | GET | API key | — | — | artist\*, album\*, _mbid_, _autocorrect_, _user_ | — | inventoried |
| `album.getTopTags` | GET | API key | — | — | artist\*, album\*, _autocorrect_, _mbid_ | — | inventoried |
| `album.removeTag` | POST | API key, signature, session | yes | — | artist, album, tag | — | inventoried |
| `album.search` | GET | API key | — | page, limit | _limit_, _page_, album | — | inventoried |
| **artist** | | | | | | | |
| `artist.addTags` | POST | API key, signature, session | yes | — | artist, tags | — | inventoried |
| `artist.getCorrection` | GET | API key | — | — | artist | — | inventoried |
| `artist.getInfo` | GET | API key | — | — | artist\*, _mbid_, _lang_, _autocorrect_, _username_ | — | inventoried |
| `artist.getSimilar` | GET | API key | — | limit | _limit_, artist\*, _autocorrect_, _mbid_ | — | inventoried |
| `artist.getTags` | GET | API key | — | — | artist\*, _mbid_, _user_, _autocorrect_ | — | inventoried |
| `artist.getTopAlbums` | GET | API key | — | page, limit | artist\*, _mbid_, _autocorrect_, _page_, _limit_ | — | inventoried |
| `artist.getTopTags` | GET | API key | — | — | artist\*, _mbid_, _autocorrect_ | — | inventoried |
| `artist.getTopTracks` | GET | API key | — | page, limit | artist\*, _mbid_, _autocorrect_, _page_, _limit_ | — | inventoried |
| `artist.removeTag` | POST | API key, signature, session | yes | — | artist, tag | — | inventoried |
| `artist.search` | GET | API key | — | page, limit | _limit_, _page_, artist | — | inventoried |
| **auth** | | | | | | | |
| `auth.getMobileSession` | POST | API key, signature | — | — | password, username | — | inventoried |
| `auth.getSession` | GET | API key, signature | — | — | token | — | inventoried |
| `auth.getToken` | GET | API key, signature | — | — | — | — | inventoried |
| **chart** | | | | | | | |
| `chart.getTopArtists` | GET | API key | — | page, limit | _page_, _limit_ | — | inventoried |
| `chart.getTopTags` | GET | API key | — | page, limit | _page_, _limit_ | — | inventoried |
| `chart.getTopTracks` | GET | API key | — | page, limit | _page_, _limit_ | — | inventoried |
| **geo** | | | | | | | |
| `geo.getTopArtists` | GET | API key | — | page, limit | country, _limit_, _page_ | — | inventoried |
| `geo.getTopTracks` | GET | API key | — | page, limit | country, _location_, _limit_, _page_ | — | inventoried |
| **library** | | | | | | | |
| `library.getArtists` | GET | API key | — | page, limit | user, _limit_, _page_ | — | inventoried |
| **tag** | | | | | | | |
| `tag.getInfo` | GET | API key | — | — | _lang_, tag | — | inventoried |
| `tag.getSimilar` | GET | API key | — | — | tag | — | inventoried |
| `tag.getTopAlbums` | GET | API key | — | page, limit | tag, _limit_, _page_ | — | inventoried |
| `tag.getTopArtists` | GET | API key | — | page, limit | tag, _limit_, _page_ | — | inventoried |
| `tag.getTopTags` | GET | API key | — | — | — | — | inventoried |
| `tag.getTopTracks` | GET | API key | — | page, limit | tag, _limit_, _page_ | — | inventoried |
| `tag.getWeeklyChartList` | GET | API key | — | — | tag | — | inventoried |
| **track** | | | | | | | |
| `track.addTags` | POST | API key, signature, session | yes | — | artist, track, tags | — | inventoried |
| `track.getCorrection` | GET | API key | — | — | artist, track | — | inventoried |
| `track.getInfo` | GET | API key | — | — | _mbid_, track\*, artist\*, _username_, _autocorrect_ | — | inventoried |
| `track.getSimilar` | GET | API key | — | limit | track\*, artist\*, _mbid_, _autocorrect_, _limit_ | — | inventoried |
| `track.getTags` | GET | API key | — | — | artist\*, track\*, _mbid_, _autocorrect_, _user_ | — | inventoried |
| `track.getTopTags` | GET | API key | — | — | track\*, artist\*, _mbid_, _autocorrect_ | — | inventoried |
| `track.love` | POST | API key, signature, session | yes | — | track, artist | — | inventoried |
| `track.removeTag` | POST | API key, signature, session | yes | — | artist, track, tag | — | inventoried |
| `track.scrobble` | POST | API key, signature, session | yes | — | artist[i], track[i], timestamp[i], _album[i]_, _context[i]_, _streamId[i]_, _chosenByUser[i]_, _trackNumber[i]_, _mbid[i]_, _albumArtist[i]_, _duration[i]_ | — | inventoried |
| `track.search` | GET | API key | — | page, limit | _limit_, _page_, track, _artist_ | — | inventoried |
| `track.unlove` | POST | API key, signature, session | yes | — | track, artist | — | inventoried |
| `track.updateNowPlaying` | POST | API key, signature, session | yes | — | artist, track, _album_, _trackNumber_, _context_, _mbid_, _duration_, _albumArtist_ | — | inventoried |
| **user** | | | | | | | |
| `user.getFriends` | GET | API key | — | page, limit | user, _recenttracks_, _limit_, _page_ | — | inventoried |
| `user.getInfo` | GET | API key | — | — | _user_ | — | inventoried |
| `user.getLovedTracks` | GET | API key | — | page, limit | user, _limit_, _page_ | — | inventoried |
| `user.getPersonalTags` | GET | API key | — | page, limit | user, tag, taggingtype, _limit_, _page_ | — | inventoried |
| `user.getRecentTracks` | GET | API key | — | page, limit | _limit_, user, _page_, _from_, _extended_, _to_ | `model::RecentTracksPage` | fixture-verified |
| `user.getTopAlbums` | GET | API key | — | page, limit | user, _period_, _limit_, _page_ | — | inventoried |
| `user.getTopArtists` | GET | API key | — | page, limit | user, _period_, _limit_, _page_ | — | inventoried |
| `user.getTopTags` | GET | API key | — | limit | user, _limit_ | — | inventoried |
| `user.getTopTracks` | GET | API key | — | page, limit | user, _period_, _limit_, _page_ | — | inventoried |
| `user.getWeeklyAlbumChart` | GET | API key | — | — | user, _from_, _to_ | — | inventoried |
| `user.getWeeklyArtistChart` | GET | API key | — | — | user, _from_, _to_ | — | inventoried |
| `user.getWeeklyChartList` | GET | API key | — | — | user | — | inventoried |
| `user.getWeeklyTrackChart` | GET | API key | — | — | user, _from_, _to_ | — | inventoried |

<!-- END GENERATED -->

## Documentation discrepancies

Places where the official pages contradict themselves, contradict the authentication guides, or look like documentation errors. The table follows the pages except where a row below says otherwise; the snapshot fixture keeps the documented spelling next to the normalised name.

- **`autocorrect[0|1]`** (13 methods: `album.getInfo`, `album.getTags`, `album.getTopTags`, `artist.getInfo`, `artist.getSimilar`, `artist.getTags`, `artist.getTopAlbums`, `artist.getTopTags`, `artist.getTopTracks`, `track.getInfo`, `track.getSimilar`, `track.getTags`, `track.getTopTags`). The page spells the parameter name with its value set. The table lists `autocorrect`, optional; the fixture keeps `autocorrect[0|1]`.
- **`taggingtype[artist|album|track]`** (`user.getPersonalTags`). Same artefact, and here the bracket lists the accepted values. The table lists `taggingtype`, required; the accepted values are a request-level check, not part of the table.
- **`extended (0|1)`** (`user.getRecentTracks`). The page puts the value hint between the name and the label, so the label reads `(0|1) (Optional)`. The table lists `extended`, optional; the fixture keeps `extended (0|1)` as the name and `(Optional)` as the label.
- **`Required (unless mbid)]`** (the same 13 methods as `autocorrect`: `artist`, `album` and `track` on them). The label has an unbalanced closing bracket, a typo for `)`. The fixture keeps the label as written; the table treats it as conditional, with `mbid` itself optional. The page does not say which of the alternatives wins when both are sent.
- **`user` in `album.getTags`, `artist.getTags`, `track.getTags`**. Labelled optional, with the description "If called in non-authenticated mode you must specify the user to look up". The pages document no `sk`, so authenticated mode is not part of the documented method. The table lists these methods as `ApiKey`, with `user` optional; callers have to pass `user` themselves.
- **`user` in `user.getInfo`**. Labelled optional, "Defaults to the authenticated user". Same situation: no `sk` is documented, so the table is `ApiKey` and a call without a session has nothing to default to. Whether a signed call with `sk` is accepted is unverified.
- **`from` and `to` in `user.getWeeklyAlbumChart`**. The description says "See User.getChartsList for more". No such method exists; the sibling pages say `User.getWeeklyChartList`. The table lists the parameters as written and is unaffected.
- **`from` and `to` in `user.getRecentTracks`**. The descriptions say "only display scrobbles after this time" and "before this time" without saying whether either bound is inclusive. The table lists both as optional; the inclusive/exclusive rule is not documented, and the design checks it on every page instead.
- **`context[i]` and `streamId[i]` in `track.scrobble`, and `context` in `track.updateNowPlaying`**. `context` is "not public, only enabled for certain API keys". `streamId` refers to `radio.getPlaylist`, part of the deprecated Radio API that is out of scope. Both are kept in the table because the pages document them.
- **`recenttracks` in `user.getFriends`**. Optional, "Whether or not to include information about friends' recent listening". The page does not say what values it accepts, unlike `extended` and `autocorrect`. The table lists it as optional with no value set.
- **Verb of `auth.getMobileSession`**. The method page does not state a verb. The authentication spec says "This call must be a POST made over HTTPS" and the mobile guide says it "will fail if you try to use it via GET or HTTP". The table uses `Post`. `auth.getToken` and `auth.getSession` are `Get` because nothing says otherwise.
- **Order of parameters** is the order of each page, which varies between sibling methods (`limit` before `page` on some, after on others; `track` before `artist` on `track.getInfo` and after on `track.getTags`). The table keeps the page order.
