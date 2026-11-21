# Social network

Six services of a social app: profiles, posts with likes, comments, a paginated and cursor-based feed, a WebSocket notification stream and a media upload stub that answers 413 for oversized files.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Profiles (`servers/profiles.toml`) | 4001 | 8 |
| Posts (`servers/posts.toml`) | 4002 | 9 |
| Comments (`servers/comments.toml`) | 4003 | 8 |
| Feed (`servers/feed.toml`) | 4004 | 9 |
| Notifications (`servers/notifications.toml`) | 4005 | 8 |
| Media (`servers/media.toml`) | 4006 | 8 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

List posts:

```bash
curl -s http://127.0.0.1:4002/posts
```

Read the paginated feed:

```bash
curl -s 'http://127.0.0.1:4004/feed?page=2'
```

An oversized upload returns 413:

```bash
curl -si -X POST http://127.0.0.1:4006/uploads -H 'content-type: application/json' -d '{"filename":"clip.mp4","bytes":50000000}'
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
