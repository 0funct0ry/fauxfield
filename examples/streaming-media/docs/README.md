# Streaming media service

A video and music streaming backend: browse the catalog, fetch a playback manifest and static media segments, manage playlists, get recommendations, handle subscriptions and collect analytics.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Catalog (`servers/catalog.toml`) | 4001 | 9 |
| Playback (`servers/playback.toml`) | 4002 | 10 |
| Playlists (`servers/playlists.toml`) | 4003 | 9 |
| Recommendations (`servers/recommendations.toml`) | 4004 | 8 |
| Subscriptions (`servers/subscriptions.toml`) | 4005 | 9 |
| Analytics (`servers/analytics.toml`) | 4006 | 9 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

Search the catalog:

```bash
curl -s 'http://127.0.0.1:4001/search?q=harbor'
```

Fetch a playback manifest:

```bash
curl -si http://127.0.0.1:4002/manifest/12
```

Fetch a static media segment:

```bash
curl -s http://127.0.0.1:4002/media/segment1.txt
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
