use crate::exgen::*;

fn health(svc: &str) -> Rt {
    get(
        "",
        "/health",
        "Health",
        &format!(r#"{{"status":"ok","service":"{svc}"}}"#),
    )
}

pub fn ex() -> Ex {
    let catalog = Srv::new("catalog", "Catalog", 4001)
        .folders(&["Titles", "Genres", "People"])
        .ds("titles", 40, &["id:sequence", "title:one_of:Midnight Harbor|Paper Lanterns|Orbit Seven|The Quiet Valley|Neon Orchard|Copper Skies|Static Bloom|Lost Meridian", "kind:one_of:movie|series|album|podcast", "genre:one_of:drama|comedy|documentary|scifi|jazz|ambient", "year:int:1990:2025", "duration_min:int:20:180", "rating:float:1:5"])
        .ds("genres", 6, &["id:sequence", "name:one_of:Drama|Comedy|Documentary|Sci-Fi|Jazz|Ambient"])
        .routes(vec![
            crud("Titles", "/titles", "Titles", "titles"),
            crud("Genres", "/genres", "Genres", "genres"),
            get("Titles", "/search", "Search titles", &list(5, r#"{"id":{{ i + 1 }},"title":"{{ fake(kind="lorem.word") }} {{ fake(kind="lorem.word") }}","kind":"movie","rating":{{ random_int(min=1, max=5) }}.5}"#))
                .when("Missing query", 400, &[("query", "q", "absent", "")], r#"{"error":"query parameter q is required"}"#)
                .delay("40..140"),
            get("Titles", "/titles/:id/credits", "Credits", &list(3, r#"{"name":"{{ fake(kind="name") }}","role":"cast"}"#))
                .when("Unknown title", 404, &[("param", "id", "eq", "999")], r#"{"error":"title not found"}"#),
            get("Titles", "/titles/:id/similar", "Similar titles", &list(4, r#"{"id":{{ random_int(min=1, max=40) }},"title":"{{ fake(kind="lorem.word") }}"}"#)),
            get("Titles", "/new", "New releases", &list(5, r#"{"id":{{ i + 1 }},"released":"{{ fake(kind="date.past") }}"}"#)),
            get("People", "/people/:id", "Person", r#"{"id":"{{ request.params.id }}","name":"{{ fake(kind="name") }}","known_for":["{{ fake(kind="lorem.word") }}"]}"#),
            get("Genres", "/genres/:id/titles", "Titles in genre", &list(4, r#"{"id":{{ i + 1 }},"title":"{{ fake(kind="lorem.word") }}"}"#)),
            health("catalog"),
        ]);

    let playback = Srv::new("playback", "Playback", 4002)
        .folders(&["Manifests", "Sessions", "Media"])
        .routes(vec![
            get("Manifests", "/manifest/:id", "Manifest", r#"{"id":"{{ request.params.id }}","protocol":"hls","segments":[{"n":1,"uri":"http://127.0.0.1:4002/media/segment1.txt","duration_s":6},{"n":2,"uri":"http://127.0.0.1:4002/media/segment2.txt","duration_s":6}]}"#)
                .when("Not licensed", 403, &[("param", "id", "eq", "666")], r#"{"error":"title not available in your region"}"#)
                .header("Accept-Ranges", "bytes")
                .delay("30..90"),
            get("Manifests", "/manifest/:id/qualities", "Qualities", r#"[{"label":"480p","kbps":1200},{"label":"720p","kbps":3000},{"label":"1080p","kbps":6000}]"#),
            post("Sessions", "/sessions", "Start session", 201, r#"{"session":"ses_{{ uuid() }}","title":{{ request.json.title_id | default(value=0) }},"token":"tok_{{ uuid() }}"}"#)
                .when("Title missing", 422, &[("body", "$.title_id", "absent", "")], r#"{"error":"title_id is required"}"#)
                .delay("60..180"),
            get("Sessions", "/sessions/:id", "Get session", r#"{"id":"{{ request.params.id }}","position_s":{{ random_int(min=0, max=3600) }},"state":"playing"}"#)
                .when("Session expired", 410, &[("param", "id", "eq", "expired")], r#"{"error":"session expired"}"#),
            route("Sessions", "PUT", "/sessions/:id/position", "Update position", r#"{"id":"{{ request.params.id }}","position_s":{{ request.json.position_s | default(value=0) }}}"#)
                .when("Negative position", 422, &[("body", "$.position_s", "lt", "0")], r#"{"error":"position_s must be non-negative"}"#),
            route("Sessions", "DELETE", "/sessions/:id", "End session", r#"{"id":"{{ request.params.id }}","ended":true}"#),
            get("Sessions", "/subtitles/:id", "Subtitle tracks", r#"[{"lang":"en","label":"English"},{"lang":"de","label":"Deutsch"},{"lang":"es","label":"Espanol"}]"#),
            get("Sessions", "/drm/license", "License", r#"{"license":"lic_{{ uuid() }}","expires_in":3600}"#)
                .when("No token", 401, &[("header", "authorization", "absent", "")], r#"{"error":"missing token"}"#),
            statics("Media", "/media/*", "Static media", "files/media"),
            health("playback"),
        ]);

    let playlists = Srv::new("playlists", "Playlists", 4003)
        .folders(&["Playlists", "Items"])
        .ds("playlists", 12, &["id:sequence", "name:one_of:Weekend Mix|Focus|Late Night|Road Trip|Family Movie Night|Workout", "owner:fake:internet.username", "items:int:0:50", "public:bool"])
        .routes(vec![
            crud("Playlists", "/playlists", "Playlists", "playlists"),
            get("Items", "/playlists/:id/items", "Playlist items", &list(4, r#"{"position":{{ i + 1 }},"title_id":{{ random_int(min=1, max=40) }}}"#))
                .when("Unknown playlist", 404, &[("param", "id", "eq", "999")], r#"{"error":"playlist not found"}"#),
            post("Items", "/playlists/:id/items", "Add item", 201, r#"{"playlist":"{{ request.params.id }}","title_id":{{ request.json.title_id | default(value=0) }},"position":{{ random_int(min=1, max=20) }}}"#)
                .when("Title missing", 422, &[("body", "$.title_id", "absent", "")], r#"{"error":"title_id is required"}"#)
                .when("Playlist full", 409, &[("param", "id", "eq", "1")], r#"{"error":"playlist is full"}"#),
            route("Items", "DELETE", "/playlists/:id/items/:title_id", "Remove item", r#"{"removed":"{{ request.params.title_id }}"}"#),
            post("Playlists", "/playlists/:id/share", "Share playlist", 200, r#"{"url":"http://127.0.0.1:4003/shared/{{ uuid() }}","expires_in":86400}"#),
            get("Playlists", "/shared/:token", "Shared playlist", r#"{"token":"{{ request.params.token }}","name":"{{ fake(kind="lorem.word") }} mix","items":{{ random_int(min=3, max=30) }}}"#),
            post("Playlists", "/playlists/:id/shuffle", "Shuffle", 200, r#"{"playlist":"{{ request.params.id }}","order":[3,1,4,2]}"#),
            get("Playlists", "/featured", "Featured playlists", &list(3, r#"{"id":{{ i + 1 }},"name":"{{ fake(kind="lorem.word") }} picks"}"#)),
            health("playlists"),
        ]);

    let recs = Srv::new("recommendations", "Recommendations", 4004)
        .folders(&["For You", "Trending"])
        .ds("signals", 20, &["id:sequence", "user_id:int:1:30", "title_id:int:1:40", "weight:float:0:1"])
        .routes(vec![
            get("For You", "/recommendations", "For you", &list(6, r#"{"title_id":{{ random_int(min=1, max=40) }},"score":0.{{ random_int(min=50, max=99) }},"reason":"{{ fake(kind="lorem.sentence") }}"}"#))
                .when("Unknown user", 404, &[("query", "user", "eq", "0")], r#"{"error":"user not found"}"#)
                .delay("80..220"),
            get("For You", "/recommendations/:user", "For a user", &list(4, r#"{"title_id":{{ random_int(min=1, max=40) }},"score":0.{{ random_int(min=50, max=99) }}}"#)),
            get("For You", "/continue/:user", "Continue watching", &list(3, r#"{"title_id":{{ random_int(min=1, max=40) }},"progress":{{ random_int(min=5, max=95) }}}"#)),
            get("Trending", "/trending", "Trending", &list(5, r#"{"rank":{{ i + 1 }},"title_id":{{ random_int(min=1, max=40) }},"views":{{ random_int(min=1000, max=90000) }}}"#)),
            get("Trending", "/trending/:genre", "Trending in genre", &list(3, r#"{"rank":{{ i + 1 }},"genre":"{{ request.params.genre }}"}"#)),
            post("For You", "/feedback", "Send feedback", 202, r#"{"id":"fb_{{ uuid() }}","title_id":{{ request.json.title_id | default(value=0) }},"signal":"{{ request.json.signal | default(value='like') }}"}"#)
                .when("Title missing", 422, &[("body", "$.title_id", "absent", "")], r#"{"error":"title_id is required"}"#),
            crud("For You", "/signals", "Signals", "signals"),
            health("recommendations"),
        ]);

    let subs = Srv::new("subscriptions", "Subscriptions", 4005)
        .var("trial_days", "14")
        .folders(&["Plans", "Subscriptions", "Billing"])
        .ds("subscriptions", 15, &["id:sequence", "user:fake:name", "plan:one_of:free|standard|premium", "status:one_of:active|trial|paused|cancelled", "renews:date"])
        .routes(vec![
            get("Plans", "/plans", "Plans", r#"[{"id":"free","price":0},{"id":"standard","price":8.99},{"id":"premium","price":14.99}]"#),
            get("Plans", "/plans/:id", "Get plan", r#"{"id":"{{ request.params.id }}","trial_days":{{ vars.trial_days }},"streams":2}"#)
                .when("Unknown plan", 404, &[("param", "id", "eq", "gold")], r#"{"error":"plan not found"}"#),
            crud("Subscriptions", "/subscriptions", "Subscriptions", "subscriptions"),
            post("Subscriptions", "/subscriptions/:id/upgrade", "Upgrade", 200, r#"{"id":"{{ request.params.id }}","plan":"{{ request.json.plan | default(value='premium') }}","effective":"immediately"}"#)
                .when("Already on plan", 409, &[("body", "$.plan", "eq", "free")], r#"{"error":"already on the free plan"}"#),
            post("Subscriptions", "/subscriptions/:id/pause", "Pause", 200, r#"{"id":"{{ request.params.id }}","status":"paused"}"#),
            post("Subscriptions", "/subscriptions/:id/cancel", "Cancel", 200, r#"{"id":"{{ request.params.id }}","status":"cancelled"}"#)
                .when("Not found", 404, &[("param", "id", "eq", "999")], r#"{"error":"subscription not found"}"#),
            get("Billing", "/invoices", "Invoices", &list(3, r#"{"id":"inv_{{ uuid() }}","amount":{{ random_int(min=8, max=15) }}.99,"paid":true}"#)),
            post("Billing", "/billing/method", "Set payment method", 200, r#"{"brand":"card","last4":"{{ fake(kind="number.digit") }}{{ fake(kind="number.digit") }}{{ fake(kind="number.digit") }}{{ fake(kind="number.digit") }}"}"#)
                .when("Card missing", 422, &[("body", "$.card", "absent", "")], r#"{"error":"card is required"}"#)
                .delay("100..300"),
            health("subscriptions"),
        ]);

    let analytics = Srv::new("analytics", "Analytics", 4006)
        .folders(&["Events", "Reports"])
        .ds("plays", 40, &["id:sequence", "title_id:int:1:40", "user_id:int:1:30", "seconds:int:10:7200", "device:one_of:tv|phone|web|tablet", "at:datetime"])
        .routes(vec![
            crud("Events", "/plays", "Plays", "plays"),
            post("Events", "/events", "Track event", 202, r#"{"accepted":true,"event":"{{ request.json.type | default(value='') }}","id":"evt_{{ uuid() }}"}"#)
                .when("Type missing", 422, &[("body", "$.type", "absent", "")], r#"{"error":"type is required"}"#)
                .when("Payload too large", 413, &[("body", "$.size", "gt", "65536")], r#"{"error":"event too large"}"#),
            get("Reports", "/reports/daily", "Daily report", r#"{"date":"{{ fake(kind="date.past") }}","plays":{{ random_int(min=1000, max=90000) }},"unique_users":{{ random_int(min=300, max=9000) }}}"#)
                .delay("100..400"),
            get("Reports", "/reports/top-titles", "Top titles", &list(5, r#"{"rank":{{ i + 1 }},"title_id":{{ random_int(min=1, max=40) }},"plays":{{ random_int(min=100, max=9000) }}}"#)),
            get("Reports", "/reports/devices", "Device split", r#"{"tv":48,"phone":27,"web":18,"tablet":7}"#),
            get("Reports", "/reports/retention", "Retention", r#"[{"day":1,"rate":0.82},{"day":7,"rate":0.54},{"day":30,"rate":0.31}]"#)
                .when("Bad cohort", 400, &[("query", "cohort", "eq", "invalid")], r#"{"error":"unknown cohort"}"#),
            get("Reports", "/reports/:name", "Named report", r#"{"report":"{{ request.params.name }}","rows":{{ random_int(min=5, max=100) }}}"#),
            get("Events", "/realtime", "Realtime viewers", r#"{"viewers":{{ random_int(min=100, max=5000) }},"at":"{{ now() }}"}"#),
            health("analytics"),
        ]);

    Ex {
        name: "streaming-media",
        title: "Streaming media service",
        story: "A video and music streaming backend: browse the catalog, fetch a playback manifest and static media segments, manage playlists, get recommendations, handle subscriptions and collect analytics.",
        servers: vec![catalog, playback, playlists, recs, subs, analytics],
        curls: vec![
            ("Search the catalog", "curl -s 'http://127.0.0.1:4001/search?q=harbor'".into()),
            ("Fetch a playback manifest", "curl -si http://127.0.0.1:4002/manifest/12".into()),
            ("Fetch a static media segment", "curl -s http://127.0.0.1:4002/media/segment1.txt".into()),
        ],
        files: vec![
            ("files/media/index.html", "<!doctype html>\n<html><head><title>Media</title></head><body><h1>Demo media</h1><p>Synthetic segments for the playback Server.</p></body></html>\n".into()),
            ("files/media/segment1.txt", "SEGMENT 1\nsynthetic media payload\n0123456789abcdef0123456789abcdef\n".into()),
            ("files/media/segment2.txt", "SEGMENT 2\nsynthetic media payload\nfedcba9876543210fedcba9876543210\n".into()),
        ],
    }
}
