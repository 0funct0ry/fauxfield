use crate::exgen::*;

pub fn ex() -> Ex {
    let profiles = Srv::new("profiles", "Profiles", 4001)
        .folders(&["Profiles", "Follows"])
        .ds("profiles", 30, &["id:sequence", "handle:fake:internet.username", "name:fake:name", "bio:fake:lorem.sentence", "followers:int:0:5000", "verified:bool"])
        .routes(vec![
            crud("Profiles", "/profiles", "Profiles", "profiles"),
            get("Profiles", "/profiles/by-handle/:handle", "Profile by handle", r#"{"handle":"{{ request.params.handle }}","name":"{{ fake(kind="name") }}","bio":"{{ fake(kind="lorem.sentence") }}","followers":{{ random_int(min=0, max=5000) }}}"#)
                .when("Unknown handle", 404, &[("param", "handle", "eq", "ghost")], r#"{"error":"profile not found"}"#)
                .when("Suspended", 403, &[("param", "handle", "starts_with", "banned")], r#"{"error":"profile suspended"}"#),
            get("Follows", "/profiles/:handle/followers", "Followers", &list(4, r#"{"handle":"{{ fake(kind="internet.username") }}","name":"{{ fake(kind="name") }}"}"#))
                .delay("30..90"),
            get("Follows", "/profiles/:handle/following", "Following", &list(3, r#"{"handle":"{{ fake(kind="internet.username") }}"}"#)),
            post("Follows", "/profiles/:handle/follow", "Follow", 200, r#"{"following":"{{ request.params.handle }}","ok":true}"#)
                .when("Cannot follow self", 422, &[("header", "x-handle", "eq", "self")], r#"{"error":"cannot follow yourself"}"#),
            route("Follows", "DELETE", "/profiles/:handle/follow", "Unfollow", r#"{"unfollowed":"{{ request.params.handle }}"}"#),
            get("Profiles", "/profiles/suggested", "Suggested profiles", &list(3, r#"{"handle":"{{ fake(kind="internet.username") }}","reason":"followed by {{ fake(kind="name.first") }}"}"#)),
            get("", "/health", "Health", r#"{"status":"ok","service":"profiles"}"#),
        ]);

    let posts = Srv::new("posts", "Posts", 4002)
        .folders(&["Posts", "Likes"])
        .ds("posts", 40, &["id:sequence", "author:fake:internet.username", "text:fake:lorem.sentence", "likes:int:0:900", "created_at:datetime"])
        .routes(vec![
            crud("Posts", "/posts", "Posts", "posts"),
            post("Likes", "/posts/:id/like", "Like post", 200, r#"{"post_id":"{{ request.params.id }}","liked":true,"likes":{{ random_int(min=1, max=900) }}}"#)
                .when("Post deleted", 410, &[("param", "id", "eq", "0")], r#"{"error":"post was deleted"}"#),
            route("Likes", "DELETE", "/posts/:id/like", "Unlike post", r#"{"post_id":"{{ request.params.id }}","liked":false}"#),
            get("Likes", "/posts/:id/likes", "Who liked", &list(4, r#"{"handle":"{{ fake(kind="internet.username") }}","at":"{{ fake(kind="date.past") }}"}"#))
                .delay("20..70"),
            post("Posts", "/posts/:id/share", "Share post", 201, r#"{"share_id":"shr_{{ uuid() }}","post_id":"{{ request.params.id }}"}"#),
            post("Posts", "/posts/drafts", "Save draft", 201, r#"{"draft_id":"drf_{{ uuid() }}","text":"{{ request.json.text | default(value='') }}"}"#)
                .when("Text too long", 413, &[("body", "$.length", "gt", "280")], r#"{"error":"post exceeds 280 characters"}"#)
                .when("Text missing", 422, &[("body", "$.text", "absent", "")], r#"{"error":"text is required"}"#),
            get("Posts", "/posts/trending", "Trending posts", &list(3, r#"{"id":{{ i + 1 }},"text":"{{ fake(kind="lorem.sentence") }}","likes":{{ random_int(min=100, max=900) }}}"#)),
            get("Posts", "/tags/:tag/posts", "Posts by tag", &list(3, r#"{"id":{{ i + 1 }},"text":"{{ fake(kind="lorem.sentence") }}","tag":"{{ request.params.tag }}"}"#)),
            get("", "/health", "Health", r#"{"status":"ok","service":"posts"}"#),
        ]);

    let comments = Srv::new("comments", "Comments", 4003)
        .folders(&["Comments", "Moderation"])
        .ds("comments", 40, &["id:sequence", "post_id:int:1:40", "author:fake:internet.username", "text:fake:lorem.sentence", "created_at:datetime"])
        .routes(vec![
            crud("Comments", "/comments", "Comments", "comments"),
            get("Comments", "/posts/:id/comments", "Comments on post", &list(4, r#"{"id":{{ i + 1 }},"author":"{{ fake(kind="internet.username") }}","text":"{{ fake(kind="lorem.sentence") }}"}"#))
                .when("Comments disabled", 403, &[("param", "id", "eq", "13")], r#"{"error":"comments are disabled for this post"}"#),
            post("Comments", "/posts/:id/comments", "Add comment", 201, r#"{"id":"cmt_{{ uuid() }}","post_id":"{{ request.params.id }}","text":"{{ request.json.text | default(value='') }}"}"#)
                .when("Empty comment", 422, &[("body", "$.text", "absent", "")], r#"{"error":"text is required"}"#)
                .when("Spam blocked", 429, &[("body", "$.text", "contains", "buy now")], r#"{"error":"slow down"}"#)
                .delay("40..140"),
            get("Comments", "/comments/:id/replies", "Replies", &list(2, r#"{"id":{{ i + 1 }},"text":"{{ fake(kind="lorem.sentence") }}"}"#)),
            post("Moderation", "/comments/:id/report", "Report comment", 202, r#"{"report_id":"rep_{{ uuid() }}","comment":"{{ request.params.id }}","status":"queued"}"#)
                .when("Reason missing", 422, &[("body", "$.reason", "absent", "")], r#"{"error":"reason is required"}"#),
            post("Moderation", "/comments/:id/hide", "Hide comment", 200, r#"{"comment":"{{ request.params.id }}","hidden":true}"#)
                .when("Moderator only", 403, &[("header", "x-role", "ne", "moderator")], r#"{"error":"moderator role required"}"#),
            get("Moderation", "/moderation/queue", "Moderation queue", &list(3, r#"{"report_id":"rep_{{ uuid() }}","reason":"spam"}"#)),
            get("", "/health", "Health", r#"{"status":"ok","service":"comments"}"#),
        ]);

    let feed = Srv::new("feed", "Feed", 4004)
        .folders(&["Feed", "Discover"])
        .ds("topics", 8, &["id:sequence", "name:one_of:music|travel|food|tech|sports|art|books|film", "followers:int:100:9000"])
        .routes(vec![
            get("Feed", "/feed", "Home feed (paginated)", r#"{"items":[{"id":"{{ uuid() }}","author":"{{ fake(kind="internet.username") }}","text":"{{ fake(kind="lorem.sentence") }}"},{"id":"{{ uuid() }}","author":"{{ fake(kind="internet.username") }}","text":"{{ fake(kind="lorem.sentence") }}"},{"id":"{{ uuid() }}","author":"{{ fake(kind="internet.username") }}","text":"{{ fake(kind="lorem.sentence") }}"}],"page":{{ request.query.page | default(value=1) }},"next_cursor":"cur_{{ uuid() }}"}"#)
                .when("Page out of range", 400, &[("query", "page", "gt", "50")], r#"{"error":"page out of range"}"#)
                .delay("40..130"),
            get("Feed", "/feed/cursor", "Feed by cursor", r#"{"items":[{"id":"{{ uuid() }}","text":"{{ fake(kind="lorem.sentence") }}"},{"id":"{{ uuid() }}","text":"{{ fake(kind="lorem.sentence") }}"}],"cursor":"{{ request.query.cursor | default(value='start') }}","next_cursor":"cur_{{ uuid() }}"}"#)
                .when("Bad cursor", 400, &[("query", "cursor", "starts_with", "bad")], r#"{"error":"invalid cursor"}"#),
            get("Feed", "/feed/following", "Following feed", &list(3, r#"{"id":"{{ uuid() }}","author":"{{ fake(kind="internet.username") }}","text":"{{ fake(kind="lorem.sentence") }}"}"#))
                .when("Sign in required", 401, &[("header", "authorization", "absent", "")], r#"{"error":"sign in to see your following feed"}"#),
            get("Feed", "/feed/count", "Unread count", r#"{"unread":{{ random_int(min=0, max=40) }}}"#),
            crud("Discover", "/topics", "Topics", "topics"),
            get("Discover", "/topics/:id/feed", "Topic feed", &list(3, r#"{"id":"{{ uuid() }}","text":"{{ fake(kind="lorem.sentence") }}","topic":"{{ request.params.id }}"}"#)),
            get("Discover", "/discover", "Discover", &list(4, r#"{"handle":"{{ fake(kind="internet.username") }}","topic":"{{ fake(kind="lorem.word") }}"}"#))
                .delay("60..200"),
            post("Feed", "/feed/seen", "Mark seen", 204, r#"{"ok":true}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"feed"}"#),
        ]);

    let notifications = Srv::new("notifications", "Notifications", 4005)
        .folders(&["Notifications", "Streams"])
        .ds("notifications", 20, &["id:sequence", "kind:one_of:like|comment|follow|mention", "actor:fake:internet.username", "read:bool", "created_at:datetime"])
        .routes(vec![
            crud("Notifications", "/notifications", "Notifications", "notifications"),
            ws("Streams", "/stream", "Notification stream", vec![
                ("connect", "", "", r#"{"type":"hello","message":"connected to notifications"}"#.into()),
                ("interval", "3s", "", r#"{"type":"notification","id":"{{ uuid() }}","kind":"like","actor":"{{ fake(kind="internet.username") }}"}"#.into()),
                ("message", "", "ping", r#"{"type":"pong","at":"{{ now() }}"}"#.into()),
            ]),
            get("Notifications", "/notifications/unread", "Unread", &list(3, r#"{"id":{{ i + 1 }},"kind":"comment","actor":"{{ fake(kind="internet.username") }}"}"#))
                .delay("20..60"),
            post("Notifications", "/notifications/read", "Mark all read", 200, r#"{"marked":{{ random_int(min=1, max=20) }}}"#),
            post("Notifications", "/notifications/:id/read", "Mark one read", 200, r#"{"id":"{{ request.params.id }}","read":true}"#)
                .when("Not found", 404, &[("param", "id", "eq", "0")], r#"{"error":"notification not found"}"#),
            get("Notifications", "/preferences", "Preferences", r#"{"likes":true,"comments":true,"follows":false,"email_digest":"weekly"}"#),
            route("Notifications", "PUT", "/preferences", "Update preferences", r#"{"updated":true}"#)
                .when("Invalid digest", 422, &[("body", "$.email_digest", "eq", "hourly")], r#"{"error":"digest must be daily or weekly"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"notifications"}"#),
        ]);

    let media = Srv::new("media", "Media", 4006)
        .folders(&["Uploads", "Library"])
        .ds("media", 15, &["id:sequence", "filename:regex:img_[0-9]{4}\\.jpg", "kind:one_of:image|video|audio", "bytes:int:10000:900000", "owner:fake:internet.username"])
        .routes(vec![
            crud("Library", "/media", "Media library", "media"),
            post("Uploads", "/uploads", "Upload stub", 201, r#"{"media_id":"med_{{ uuid() }}","filename":"{{ request.json.filename | default(value='upload.jpg') }}","status":"processing"}"#)
                .when("File too large", 413, &[("body", "$.bytes", "gt", "10000000")], r#"{"error":"file exceeds 10 MB limit"}"#)
                .when("Unsupported type", 415, &[("body", "$.filename", "ends_with", ".exe")], r#"{"error":"unsupported media type"}"#)
                .delay("150..400"),
            post("Uploads", "/uploads/presign", "Presign upload", 200, r#"{"url":"http://127.0.0.1:4006/uploads","expires_in":300,"token":"up_{{ uuid() }}"}"#),
            get("Uploads", "/uploads/:id/status", "Upload status", r#"{"id":"{{ request.params.id }}","status":"ready","variants":["thumb","medium","full"]}"#)
                .when("Still processing", 202, &[("param", "id", "starts_with", "new")], r#"{"id":"new","status":"processing"}"#),
            get("Library", "/media/:id/thumbnail", "Thumbnail", r#"{"id":"{{ request.params.id }}","url":"http://127.0.0.1:4006/thumbs/{{ request.params.id }}.jpg","width":320,"height":240}"#)
                .when("Thumb missing", 404, &[("param", "id", "eq", "0")], r#"{"error":"thumbnail not found"}"#),
            get("Library", "/albums", "Albums", &list(3, r#"{"id":{{ i + 1 }},"title":"Album {{ i + 1 }}","items":{{ random_int(min=3, max=40) }}}"#)),
            route("Library", "DELETE", "/media/:id/purge", "Purge media", r#"{"purged":"{{ request.params.id }}"}"#)
                .when("Forbidden", 403, &[("header", "x-role", "absent", "")], r#"{"error":"role header required"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"media"}"#),
        ]);

    Ex {
        name: "social",
        title: "Social network",
        story: "Six services of a social app: profiles, posts with likes, comments, a paginated and cursor-based feed, a WebSocket notification stream and a media upload stub that answers 413 for oversized files.",
        servers: vec![profiles, posts, comments, feed, notifications, media],
        curls: vec![
            ("List posts", "curl -s http://127.0.0.1:4002/posts".into()),
            ("Read the paginated feed", "curl -s 'http://127.0.0.1:4004/feed?page=2'".into()),
            ("An oversized upload returns 413", "curl -si -X POST http://127.0.0.1:4006/uploads -H 'content-type: application/json' -d '{\"filename\":\"clip.mp4\",\"bytes\":50000000}'".into()),
        ],
        files: vec![],
    }
}
