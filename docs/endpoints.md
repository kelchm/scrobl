# Endpoints

Every method in the official Last.fm API index, with the verb, credentials, paging and parameters its documentation gives, and the level scrobl has reached for it. The levels (`inventoried`, `request-verified`, `typed-derived`, `fixture-verified`, `live-verified`) are defined under Coverage in [`design.md`](design.md), and this table is the release claim: a method is only as verified as its row says.

The rows come from the method table in `crates/scrobl/src/protocol/methods.rs` and are checked against a snapshot of the official method pages dated 2026-10-05 (`crates/scrobl/fixtures/official/method-parameters.json`). Run `SCROBL_BLESS=1 cargo test --test inventory` to regenerate the block below; do not edit it by hand.

Credentials: every method needs an API key. A signature is an `api_sig` made with the API secret. A session is a user's session key, sent as `sk`. In the Parameters column, required parameters are plain, `name\*` is required unless an alternative such as `mbid` is given, and _italic_ names are optional. `name[i]` takes a batch index (`artist[0]`, `artist[1]`, ...). `api_key`, `api_sig`, `sk`, `method`, `format` and `callback` are implied by the credentials and never listed.

Out of scope, because the official page marks it deprecated or because it is only another encoding of the same methods: the Radio API, the Playlists API, Submissions Protocol 1.2.1, XML-RPC and XML output. Website scraping and history editing are out of scope permanently.

Not in the index at the snapshot date, and so not in the table: `track.ban`, `track.unban`, `user.getArtistTracks` and `user.getNewReleases`, which older documentation and other libraries list.

<!-- BEGIN GENERATED -->

| Method | Verb | Credentials | Write | Paging | Parameters | Typed model | Status |
|---|---|---|---|---|---|---|---|
| **album** | | | | | | | |
| `album.addTags` | POST | API key, signature, session | yes | — | artist, album, tags | — | inventoried |
| `album.getInfo` | GET | API key | — | — | artist\*, album\*, _mbid_, _autocorrect_, _username_, _lang_ | `model::AlbumInfo` | live-verified |
| `album.getTags` | GET | API key | — | — | artist\*, album\*, _mbid_, _autocorrect_, _user_ | `model::List<Tag>` | live-verified |
| `album.getTopTags` | GET | API key | — | — | artist\*, album\*, _autocorrect_, _mbid_ | `model::List<TopTag>` | live-verified |
| `album.removeTag` | POST | API key, signature, session | yes | — | artist, album, tag | — | inventoried |
| `album.search` | GET | API key | — | page, limit | _limit_, _page_, album | `model::Page<Album>` | live-verified |
| **artist** | | | | | | | |
| `artist.addTags` | POST | API key, signature, session | yes | — | artist, tags | — | inventoried |
| `artist.getCorrection` | GET | API key | — | — | artist | `model::Artist` | live-verified |
| `artist.getInfo` | GET | API key | — | — | artist\*, _mbid_, _lang_, _autocorrect_, _username_ | `model::ArtistInfo` | live-verified |
| `artist.getSimilar` | GET | API key | — | limit | _limit_, artist\*, _autocorrect_, _mbid_ | `model::List<SimilarArtist>` | live-verified |
| `artist.getTags` | GET | API key | — | — | artist\*, _mbid_, _user_, _autocorrect_ | `model::List<Tag>` | live-verified |
| `artist.getTopAlbums` | GET | API key | — | page, limit | artist\*, _mbid_, _autocorrect_, _page_, _limit_ | `model::Page<Album>` | live-verified |
| `artist.getTopTags` | GET | API key | — | — | artist\*, _mbid_, _autocorrect_ | `model::List<TopTag>` | live-verified |
| `artist.getTopTracks` | GET | API key | — | page, limit | artist\*, _mbid_, _autocorrect_, _page_, _limit_ | `model::Page<Track>` | live-verified |
| `artist.removeTag` | POST | API key, signature, session | yes | — | artist, tag | — | inventoried |
| `artist.search` | GET | API key | — | page, limit | _limit_, _page_, artist | `model::Page<Artist>` | live-verified |
| **auth** | | | | | | | |
| `auth.getMobileSession` | POST | API key, signature | — | — | password, username | `model::Session` | typed-derived |
| `auth.getSession` | GET | API key, signature | — | — | token | `model::Session` | live-verified |
| `auth.getToken` | GET | API key, signature | — | — | — | `AuthToken` | live-verified |
| **chart** | | | | | | | |
| `chart.getTopArtists` | GET | API key | — | page, limit | _page_, _limit_ | `model::Page<Artist>` | live-verified |
| `chart.getTopTags` | GET | API key | — | page, limit | _page_, _limit_ | `model::List<Tag>` | live-verified |
| `chart.getTopTracks` | GET | API key | — | page, limit | _page_, _limit_ | `model::Page<Track>` | live-verified |
| **geo** | | | | | | | |
| `geo.getTopArtists` | GET | API key | — | page, limit | country, _limit_, _page_ | `model::Page<CountryArtist>` | live-verified |
| `geo.getTopTracks` | GET | API key | — | page, limit | country, _location_, _limit_, _page_ | `model::Page<CountryTrack>` | live-verified |
| **library** | | | | | | | |
| `library.getArtists` | GET | API key | — | page, limit | user, _limit_, _page_ | `model::Page<UserArtist>` | live-verified |
| **tag** | | | | | | | |
| `tag.getInfo` | GET | API key | — | — | _lang_, tag | `model::TagInfo` | live-verified |
| `tag.getSimilar` | GET | API key | — | — | tag | `model::List<Tag>` | typed-derived |
| `tag.getTopAlbums` | GET | API key | — | page, limit | tag, _limit_, _page_ | `model::Page<Album>` | live-verified |
| `tag.getTopArtists` | GET | API key | — | page, limit | tag, _limit_, _page_ | `model::Page<Artist>` | live-verified |
| `tag.getTopTags` | GET | API key | — | — | — | `model::List<Tag>` | live-verified |
| `tag.getTopTracks` | GET | API key | — | page, limit | tag, _limit_, _page_ | `model::Page<Track>` | live-verified |
| `tag.getWeeklyChartList` | GET | API key | — | — | tag | `model::List<Window>` | live-verified |
| **track** | | | | | | | |
| `track.addTags` | POST | API key, signature, session | yes | — | artist, track, tags | — | inventoried |
| `track.getCorrection` | GET | API key | — | — | artist, track | `model::Track` | live-verified |
| `track.getInfo` | GET | API key | — | — | _mbid_, track\*, artist\*, _username_, _autocorrect_ | `model::TrackInfo` | live-verified |
| `track.getSimilar` | GET | API key | — | limit | track\*, artist\*, _mbid_, _autocorrect_, _limit_ | `model::List<SimilarTrack>` | live-verified |
| `track.getTags` | GET | API key | — | — | artist\*, track\*, _mbid_, _autocorrect_, _user_ | `model::List<Tag>` | live-verified |
| `track.getTopTags` | GET | API key | — | — | track\*, artist\*, _mbid_, _autocorrect_ | `model::List<TopTag>` | live-verified |
| `track.love` | POST | API key, signature, session | yes | — | track, artist | — | inventoried |
| `track.removeTag` | POST | API key, signature, session | yes | — | artist, track, tag | — | inventoried |
| `track.scrobble` | POST | API key, signature, session | yes | — | artist[i], track[i], timestamp[i], _album[i]_, _context[i]_, _streamId[i]_, _chosenByUser[i]_, _trackNumber[i]_, _mbid[i]_, _albumArtist[i]_, _duration[i]_ | — | inventoried |
| `track.search` | GET | API key | — | page, limit | _limit_, _page_, track, _artist_ | `model::Page<Track>` | live-verified |
| `track.unlove` | POST | API key, signature, session | yes | — | track, artist | — | inventoried |
| `track.updateNowPlaying` | POST | API key, signature, session | yes | — | artist, track, _album_, _trackNumber_, _context_, _mbid_, _duration_, _albumArtist_ | — | inventoried |
| **user** | | | | | | | |
| `user.getFriends` | GET | API key | — | page, limit | user, _recenttracks_, _limit_, _page_ | `model::Page<User>` | live-verified |
| `user.getInfo` | GET | API key | — | — | _user_ | `model::User` | live-verified |
| `user.getLovedTracks` | GET | API key | — | page, limit | user, _limit_, _page_ | `model::Page<LovedTrack>` | live-verified |
| `user.getPersonalTags` | GET | API key | — | page, limit | user, tag, taggingtype, _limit_, _page_ | `model::Page` of `Artist`, `Album` or `Track` | live-verified |
| `user.getRecentTracks` | GET | API key | — | page, limit | _limit_, user, _page_, _from_, _extended_, _to_ | `model::HistoryPage` | live-verified |
| `user.getTopAlbums` | GET | API key | — | page, limit | user, _period_, _limit_, _page_ | `model::Page<UserAlbum>` | live-verified |
| `user.getTopArtists` | GET | API key | — | page, limit | user, _period_, _limit_, _page_ | `model::Page<UserArtist>` | live-verified |
| `user.getTopTags` | GET | API key | — | limit | user, _limit_ | `model::List<UserTag>` | live-verified |
| `user.getTopTracks` | GET | API key | — | page, limit | user, _period_, _limit_, _page_ | `model::Page<UserTrack>` | live-verified |
| `user.getWeeklyAlbumChart` | GET | API key | — | — | user, _from_, _to_ | `model::List<UserAlbum>` | live-verified |
| `user.getWeeklyArtistChart` | GET | API key | — | — | user, _from_, _to_ | `model::List<UserArtist>` | live-verified |
| `user.getWeeklyChartList` | GET | API key | — | — | user | `model::List<Window>` | live-verified |
| `user.getWeeklyTrackChart` | GET | API key | — | — | user, _from_, _to_ | `model::List<UserTrack>` | live-verified |

<!-- END GENERATED -->

## Documentation discrepancies

Places where the official pages contradict themselves, contradict the authentication guides, or look like documentation errors. The table follows the pages except where a row below says otherwise; the snapshot fixture keeps the documented spelling next to the normalised name.

This list is about the pages. Where the live service was seen to do something other than its page says, the table still follows the page, and what was seen is in [`design.md`](design.md#what-the-live-passes-showed). `chart.getTopTags` is one: its page documents `page`, and the service sent the same rows for every page.

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
