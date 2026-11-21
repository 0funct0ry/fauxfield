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
    let devices = Srv::new("devices", "Devices", 4001)
        .var("region", "eu-west")
        .folders(&["Devices", "Groups"])
        .ds("devices", 30, &["id:sequence", "name:one_of:Tracker|Gateway|Sensor Hub|Pump Monitor|Door Node|Cold Chain Probe", "serial:regex:SN-[0-9]{6}", "model:one_of:FX-100|FX-200|FX-300", "status:one_of:online|offline|maintenance", "battery:int:0:100", "last_seen:datetime"])
        .routes(vec![
            crud("Devices", "/devices", "Devices", "devices"),
            get("Devices", "/devices/:id/status", "Device status", r#"{"id":"{{ request.params.id }}","status":"online","battery":{{ random_int(min=5, max=100) }},"signal_dbm":-{{ random_int(min=40, max=95) }},"ip":"{{ fake(kind="internet.ipv4") }}"}"#)
                .when("Unknown device", 404, &[("param", "id", "eq", "999")], r#"{"error":"device not found"}"#),
            post("Devices", "/devices/:id/command", "Send command", 202, r#"{"command_id":"cmd_{{ uuid() }}","device":"{{ request.params.id }}","command":"{{ request.json.command | default(value='') }}","state":"queued"}"#)
                .when("Command missing", 422, &[("body", "$.command", "absent", "")], r#"{"error":"command is required"}"#)
                .when("Device offline", 409, &[("param", "id", "eq", "13")], r#"{"error":"device is offline"}"#)
                .delay("60..200"),
            get("Devices", "/devices/:id/config", "Device config", r#"{"id":"{{ request.params.id }}","report_interval_s":60,"region":"{{ vars.region }}","tags":["{{ fake(kind="lorem.word") }}","{{ fake(kind="lorem.word") }}"]}"#),
            route("Devices", "PUT", "/devices/:id/config", "Update config", r#"{"id":"{{ request.params.id }}","report_interval_s":{{ request.json.report_interval_s | default(value=60) }},"applied":true}"#)
                .when("Interval too small", 422, &[("body", "$.report_interval_s", "lt", "5")], r#"{"error":"report_interval_s must be at least 5"}"#),
            get("Groups", "/groups", "Groups", r#"[{"id":1,"name":"Warehouse"},{"id":2,"name":"Trucks"},{"id":3,"name":"Cold storage"}]"#),
            get("Groups", "/groups/:id/devices", "Group devices", &list(4, r#"{"id":{{ i + 1 }},"serial":"SN-{{ random_int(min=100000, max=999999) }}","name":"{{ fake(kind="lorem.word") }}"}"#))
                .delay("30..90"),
            get("Groups", "/models", "Device models", r#"["FX-100","FX-200","FX-300"]"#),
            health("devices"),
        ]);

    let telemetry = Srv::new("telemetry", "Telemetry", 4002)
        .folders(&["Readings", "Series", "Ingest"])
        .ds("readings", 50, &["id:sequence", "device_id:int:1:30", "temperature:float:-10:45", "humidity:float:10:95", "recorded_at:datetime"])
        .routes(vec![
            crud("Readings", "/readings", "Readings", "readings"),
            get("Readings", "/telemetry/:device/latest", "Latest reading", r#"{"device":"{{ request.params.device }}","temperature":{{ random_int(min=-5, max=40) }}.4,"humidity":{{ random_int(min=20, max=90) }}.1,"at":"{{ now() }}"}"#)
                .when("No data yet", 404, &[("param", "device", "eq", "0")], r#"{"error":"no readings for device"}"#),
            get("Series", "/telemetry/:device/series", "Time series", &list(12, r#"{"t":{{ i }},"temperature":{{ random_int(min=10, max=35) }}.{{ random_int(min=0, max=9) }},"humidity":{{ random_int(min=30, max=80) }}}"#))
                .when("Too many points", 400, &[("query", "points", "gt", "500")], r#"{"error":"points must be at most 500"}"#)
                .delay("50..180"),
            get("Series", "/telemetry/:device/summary", "Summary", r#"{"device":"{{ request.params.device }}","min":{{ random_int(min=-10, max=5) }},"max":{{ random_int(min=25, max=45) }},"avg":{{ random_int(min=10, max=25) }}.5,"samples":{{ random_int(min=100, max=5000) }}}"#),
            post("Ingest", "/telemetry/ingest", "Ingest reading", 202, r#"{"accepted":true,"device":{{ request.json.device_id | default(value=0) }},"id":"rd_{{ uuid() }}"}"#)
                .when("Device missing", 422, &[("body", "$.device_id", "absent", "")], r#"{"error":"device_id is required"}"#)
                .delay("10..40"),
            post("Ingest", "/telemetry/batch", "Ingest batch", 202, r#"{"accepted":{{ request.json.count | default(value=0) }},"batch":"bt_{{ uuid() }}"}"#)
                .when("Batch too large", 413, &[("body", "$.count", "gt", "1000")], r#"{"error":"batch limit is 1000 readings"}"#),
            get("Series", "/metrics", "Metric names", r#"["temperature","humidity","battery","signal_dbm"]"#),
            get("Series", "/telemetry/:device/export", "Export CSV rows", r#"{"device":"{{ request.params.device }}","format":"json","rows":3}"#),
            health("telemetry"),
        ]);

    let firmware = Srv::new("firmware", "Firmware", 4003)
        .folders(&["Versions", "Updates"])
        .ds("versions", 8, &["id:sequence", "version:regex:[1-3]\\.[0-9]\\.[0-9]", "channel:one_of:stable|beta", "released:date", "size_kb:int:512:4096"])
        .routes(vec![
            crud("Versions", "/versions", "Versions", "versions"),
            get("Versions", "/versions/latest", "Latest version", r#"{"version":"3.2.1","channel":"stable","size_kb":2048,"notes":"{{ fake(kind="lorem.sentence") }}"}"#),
            get("Versions", "/channels", "Channels", r#"["stable","beta"]"#),
            post("Updates", "/updates", "Start update job", 202, r#"{"job":"job_{{ uuid() }}","device":{{ request.json.device_id | default(value=0) }},"version":"{{ request.json.version | default(value='3.2.1') }}","state":"scheduled"}"#)
                .when("Unknown version", 404, &[("body", "$.version", "eq", "0.0.0")], r#"{"error":"firmware version not found"}"#)
                .when("Device missing", 422, &[("body", "$.device_id", "absent", "")], r#"{"error":"device_id is required"}"#)
                .delay("100..300"),
            get("Updates", "/updates/:job", "Update job", r#"{"job":"{{ request.params.job }}","state":"downloading","progress":{{ random_int(min=1, max=99) }}}"#)
                .when("Job failed", 200, &[("param", "job", "eq", "failed")], r#"{"job":"failed","state":"failed","error":"checksum mismatch"}"#),
            get("Updates", "/updates/:job/log", "Update log", &list(3, r#"{"step":{{ i + 1 }},"message":"{{ fake(kind="lorem.sentence") }}"}"#)),
            route("Updates", "DELETE", "/updates/:job", "Cancel update", r#"{"job":"{{ request.params.job }}","state":"cancelled"}"#)
                .when("Already complete", 409, &[("param", "job", "eq", "done")], r#"{"error":"update already completed"}"#),
            get("Updates", "/updates", "Recent jobs", &list(4, r#"{"job":"job_{{ uuid() }}","state":"completed","device":{{ i + 1 }}}"#)),
            health("firmware"),
        ]);

    let alerts = Srv::new("alerts", "Alerts", 4004)
        .folders(&["Alerts", "Alert Rules"])
        .ds("alerts", 25, &["id:sequence", "device_id:int:1:30", "severity:one_of:info|warning|critical", "message:lorem:6", "state:one_of:open|acked|resolved", "raised_at:datetime"])
        .routes(vec![
            crud("Alerts", "/alerts", "Alerts", "alerts"),
            post("Alerts", "/alerts/:id/ack", "Acknowledge alert", 200, r#"{"id":"{{ request.params.id }}","state":"acked","by":"{{ fake(kind="internet.username") }}"}"#)
                .when("Already resolved", 409, &[("param", "id", "lte", "3")], r#"{"error":"alert already resolved"}"#),
            post("Alerts", "/alerts/:id/resolve", "Resolve alert", 200, r#"{"id":"{{ request.params.id }}","state":"resolved"}"#)
                .when("Not found", 404, &[("param", "id", "eq", "999")], r#"{"error":"alert not found"}"#),
            get("Alerts", "/alerts/summary", "Summary", r#"{"open":{{ random_int(min=1, max=20) }},"acked":{{ random_int(min=0, max=10) }},"critical":{{ random_int(min=0, max=4) }}}"#),
            get("Alerts", "/alerts/:id/history", "Alert history", &list(3, r#"{"state":"open","at":"{{ fake(kind="date.past") }}"}"#)),
            get("Alert Rules", "/rules", "Alert rules", r#"[{"id":1,"metric":"temperature","op":"gt","threshold":40,"severity":"critical"},{"id":2,"metric":"battery","op":"lt","threshold":15,"severity":"warning"}]"#),
            post("Alert Rules", "/rules", "Create rule", 201, r#"{"id":{{ random_int(min=10, max=99) }},"metric":"{{ request.json.metric | default(value='temperature') }}","threshold":{{ request.json.threshold | default(value=0) }}}"#)
                .when("Threshold missing", 422, &[("body", "$.threshold", "absent", "")], r#"{"error":"threshold is required"}"#),
            get("Alert Rules", "/rules/:id", "Get rule", r#"{"id":"{{ request.params.id }}","metric":"temperature","op":"gt","threshold":40}"#)
                .when("Rule not found", 404, &[("param", "id", "gt", "50")], r#"{"error":"rule not found"}"#),
            health("alerts"),
        ]);

    let geofences = Srv::new("geofences", "Geofences", 4005)
        .folders(&["Fences", "Events"])
        .ds("geofences", 8, &["id:sequence", "name:one_of:Depot|Harbor|Airport|City Centre|Warehouse Yard|Service Bay", "radius_m:int:100:5000", "lat:float:47:55", "lon:float:2:14", "active:bool"])
        .routes(vec![
            crud("Fences", "/geofences", "Geofences", "geofences"),
            post("Fences", "/geofences/check", "Point in fence", 200, r#"{"lat":{{ request.json.lat | default(value=0) }},"lon":{{ request.json.lon | default(value=0) }},"inside":true,"fence":"Depot"}"#)
                .when("Coordinates missing", 422, &[("body", "$.lat", "absent", "")], r#"{"error":"lat and lon are required"}"#)
                .delay("20..80"),
            get("Fences", "/geofences/:id/devices", "Devices inside", &list(3, r#"{"device":{{ random_int(min=1, max=30) }},"since":"{{ fake(kind="date.past") }}"}"#))
                .when("Unknown fence", 404, &[("param", "id", "eq", "999")], r#"{"error":"geofence not found"}"#),
            post("Fences", "/geofences/:id/activate", "Activate fence", 200, r#"{"id":"{{ request.params.id }}","active":true}"#),
            post("Fences", "/geofences/:id/deactivate", "Deactivate fence", 200, r#"{"id":"{{ request.params.id }}","active":false}"#),
            get("Events", "/events", "Fence events", &list(5, r#"{"id":"ev_{{ uuid() }}","type":"enter","device":{{ random_int(min=1, max=30) }},"at":"{{ fake(kind="date.past") }}"}"#)),
            get("Events", "/events/:id", "Fence event", r#"{"id":"{{ request.params.id }}","type":"exit","device":{{ random_int(min=1, max=30) }}}"#),
            get("Fences", "/zones/types", "Zone types", r#"["circle","polygon"]"#),
            health("geofences"),
        ]);

    let stream = Srv::new("stream", "Stream", 4006)
        .folders(&["Sockets", "Control"])
        .routes(vec![
            ws("Sockets", "/stream/telemetry", "Telemetry stream", vec![
                ("connect", "", "", r#"{"type":"hello","channel":"telemetry"}"#.into()),
                ("interval", "2s", "", r#"{"type":"telemetry","device":{{ random_int(min=1, max=30) }},"temperature":{{ random_int(min=10, max=35) }}.2,"at":"{{ now() }}"}"#.into()),
                ("message", "", "ping", r#"{"type":"pong"}"#.into()),
            ]),
            ws("Sockets", "/stream/alerts", "Alert stream", vec![
                ("connect", "", "", r#"{"type":"hello","channel":"alerts"}"#.into()),
                ("interval", "2s", "", r#"{"type":"alert","id":"al_{{ uuid() }}","severity":"warning","message":"{{ fake(kind="lorem.sentence") }}"}"#.into()),
            ]),
            get("Control", "/stream/channels", "Channels", r#"["telemetry","alerts"]"#),
            get("Control", "/stream/status", "Status", r#"{"clients":{{ random_int(min=1, max=40) }},"messages_per_s":{{ random_int(min=1, max=50) }}}"#),
            post("Control", "/stream/subscribe", "Subscribe", 200, r#"{"subscription":"sub_{{ uuid() }}","channel":"{{ request.json.channel | default(value='') }}"}"#)
                .when("Channel missing", 422, &[("body", "$.channel", "absent", "")], r#"{"error":"channel is required"}"#)
                .when("Unknown channel", 404, &[("body", "$.channel", "eq", "nope")], r#"{"error":"unknown channel"}"#),
            route("Control", "DELETE", "/stream/subscriptions/:id", "Unsubscribe", r#"{"subscription":"{{ request.params.id }}","removed":true}"#),
            get("Control", "/stream/clients", "Clients", &list(3, r#"{"id":"cl_{{ uuid() }}","ip":"{{ fake(kind="internet.ipv4") }}"}"#)),
            health("stream"),
        ]);

    Ex {
        name: "iot-fleet",
        title: "IoT fleet management",
        story: "A fleet of connected devices: manage devices, ingest and read telemetry, roll out firmware, raise alerts, track geofences and watch a live WebSocket stream of readings.",
        servers: vec![devices, telemetry, firmware, alerts, geofences, stream],
        curls: vec![
            ("List devices", "curl -s http://127.0.0.1:4001/devices".into()),
            ("Send a command to a device", "curl -s -X POST http://127.0.0.1:4001/devices/7/command -H 'content-type: application/json' -d '{\"command\":\"reboot\"}'".into()),
            ("Latest time series (a points value above 500 returns 400)", "curl -s 'http://127.0.0.1:4002/telemetry/7/series?points=24'".into()),
        ],
        files: vec![],
    }
}
