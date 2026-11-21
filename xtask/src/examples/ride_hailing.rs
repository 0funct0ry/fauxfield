use crate::exgen::*;

pub fn ex() -> Ex {
    let riders = Srv::new("riders", "Riders", 4001)
        .folders(&["Riders", "Places"])
        .ds("riders", 30, &["id:sequence", "name:fake:name", "email:fake:internet.email", "phone:fake:phone", "rating:float:3.5:5", "joined:datetime"])
        .routes(vec![
            crud("Riders", "/riders", "Riders", "riders"),
            post("Riders", "/riders/signup", "Sign up", 201, r#"{"id":"rdr_{{ uuid() }}","email":"{{ request.json.email | default(value='new@example.test') }}","status":"active"}"#)
                .when("Email missing", 422, &[("body", "$.email", "absent", "")], r#"{"error":"email is required"}"#)
                .when("Email taken", 409, &[("body", "$.email", "eq", "taken@example.test")], r#"{"error":"email already registered"}"#),
            get("Riders", "/riders/:id/profile", "Rider profile", r#"{"id":"{{ request.params.id }}","name":"{{ fake(kind="name") }}","email":"{{ fake(kind="internet.email") }}","rating":4.{{ random_int(min=1, max=9) }}}"#)
                .when("Unknown rider", 404, &[("param", "id", "eq", "999")], r#"{"error":"rider not found"}"#),
            get("Riders", "/riders/:id/trips", "Rider trip history", &list(4, r#"{"trip_id":{{ i + 100 }},"from":"{{ fake(kind="address.street") }}","fare":{{ random_int(min=6, max=60) }}.50}"#))
                .delay("30..90"),
            get("Places", "/places/saved", "Saved places", r#"[{"label":"Home","address":"12 Maple Row"},{"label":"Work","address":"88 Foundry Street"}]"#),
            post("Places", "/places/saved", "Save place", 201, r#"{"id":"plc_{{ uuid() }}","label":"{{ request.json.label | default(value='Place') }}"}"#)
                .when("Label missing", 422, &[("body", "$.label", "absent", "")], r#"{"error":"label is required"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"riders"}"#),
        ]);

    let drivers = Srv::new("drivers", "Drivers", 4002)
        .folders(&["Drivers", "Availability", "Vehicles"])
        .ds("drivers", 25, &["id:sequence", "name:fake:name", "phone:fake:phone", "status:one_of:online|offline|on_trip", "rating:float:3.8:5", "vehicle:one_of:Sedan|Hatchback|SUV|Van"])
        .routes(vec![
            crud("Drivers", "/drivers", "Drivers", "drivers"),
            get("Drivers", "/drivers/:id/profile", "Driver profile", r#"{"id":"{{ request.params.id }}","name":"{{ fake(kind="name") }}","vehicle":{"plate":"RH-{{ random_int(min=1000, max=9999) }}","model":"Sedan"}}"#)
                .when("Driver suspended", 403, &[("param", "id", "eq", "666")], r#"{"error":"driver account suspended"}"#),
            get("Availability", "/drivers/nearby", "Nearby drivers", &list(5, r#"{"id":{{ i + 1 }},"distance_m":{{ random_int(min=100, max=2500) }},"eta_min":{{ random_int(min=1, max=9) }}}"#))
                .when("Missing location", 400, &[("query", "lat", "absent", "")], r#"{"error":"lat and lng are required"}"#)
                .delay("40..160"),
            post("Availability", "/drivers/:id/online", "Go online", 200, r#"{"id":"{{ request.params.id }}","status":"online"}"#),
            post("Availability", "/drivers/:id/offline", "Go offline", 200, r#"{"id":"{{ request.params.id }}","status":"offline"}"#),
            post("Availability", "/drivers/:id/location", "Report location", 200, r#"{"id":"{{ request.params.id }}","accepted":true,"at":"{{ now() }}"}"#)
                .when("Bad coordinates", 422, &[("body", "$.lat", "absent", "")], r#"{"error":"lat is required"}"#),
            get("Vehicles", "/vehicles/types", "Vehicle types", r#"["economy","comfort","xl","electric"]"#),
            get("Drivers", "/drivers/:id/earnings", "Earnings", r#"{"driver_id":"{{ request.params.id }}","week_total":{{ random_int(min=300, max=1500) }}.00,"trips":{{ random_int(min=20, max=90) }}}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"drivers"}"#),
        ]);

    let trips = Srv::new("trips", "Trips", 4003)
        .folders(&["Trips", "Lifecycle"])
        .ds("trips", 30, &["id:sequence", "rider_id:int:1:30", "driver_id:int:1:25", "status:one_of:requested|accepted|arrived|in_progress|completed|cancelled", "fare:float:6:80", "requested_at:datetime"])
        .routes(vec![
            crud("Trips", "/trips", "Trips", "trips"),
            post("Trips", "/trips/request", "Request trip", 201, r#"{"id":"trp_{{ uuid() }}","status":"requested","pickup":"{{ request.json.pickup | default(value='unknown') }}","eta_min":{{ random_int(min=2, max=9) }}}"#)
                .when("Pickup missing", 422, &[("body", "$.pickup", "absent", "")], r#"{"error":"pickup is required"}"#)
                .delay("100..300"),
            post("Lifecycle", "/trips/:id/accept", "Accept", 200, r#"{"id":"{{ request.params.id }}","status":"accepted"}"#)
                .when("Already cancelled", 409, &[("param", "id", "eq", "13")], r#"{"error":"trip is cancelled, cannot accept"}"#),
            post("Lifecycle", "/trips/:id/arrive", "Driver arrived", 200, r#"{"id":"{{ request.params.id }}","status":"arrived"}"#)
                .when("Not accepted yet", 409, &[("param", "id", "eq", "1")], r#"{"error":"trip must be accepted before arrival"}"#),
            post("Lifecycle", "/trips/:id/start", "Start", 200, r#"{"id":"{{ request.params.id }}","status":"in_progress","started_at":"{{ now() }}"}"#)
                .when("Not arrived yet", 409, &[("param", "id", "eq", "2")], r#"{"error":"driver has not arrived"}"#),
            post("Lifecycle", "/trips/:id/complete", "Complete", 200, r#"{"id":"{{ request.params.id }}","status":"completed","fare":{{ random_int(min=8, max=70) }}.25}"#)
                .when("Already completed", 409, &[("param", "id", "lte", "5")], r#"{"error":"trip already completed"}"#),
            post("Lifecycle", "/trips/:id/cancel", "Cancel", 200, r#"{"id":"{{ request.params.id }}","status":"cancelled","fee":0}"#)
                .when("Cancel fee after start", 200, &[("body", "$.reason", "eq", "changed_mind")], r#"{"status":"cancelled","fee":3.50}"#)
                .when("Cannot cancel in progress", 409, &[("body", "$.state", "eq", "in_progress")], r#"{"error":"trip already in progress"}"#),
            get("Trips", "/trips/:id/route", "Route", &list(4, r#"{"lat":40.{{ random_int(min=1000, max=9999) }},"lng":-73.{{ random_int(min=1000, max=9999) }}}"#)),
            get("", "/health", "Health", r#"{"status":"ok","service":"trips"}"#),
        ]);

    let pricing = Srv::new("pricing", "Pricing", 4004)
        .var("surge", "1.4")
        .var("currency", "USD")
        .folders(&["Estimates", "Surge", "Promos"])
        .routes(vec![
            post("Estimates", "/estimate", "Fare estimate", 200, r#"{"currency":"{{ vars.currency }}","surge":{{ vars.surge }},"base":4.00,"distance_km":{{ random_int(min=2, max=25) }},"estimate":{{ random_int(min=8, max=60) }}.00}"#)
                .when("Pickup missing", 422, &[("body", "$.pickup", "absent", "")], r#"{"error":"pickup is required"}"#)
                .when("Out of service area", 400, &[("body", "$.pickup", "eq", "OUTSIDE")], r#"{"error":"pickup is outside the service area"}"#)
                .delay("60..180"),
            get("Surge", "/surge", "Current surge", r#"{"multiplier":{{ vars.surge }},"reason":"high demand","updated":"{{ now() }}"}"#),
            get("Surge", "/surge/zones", "Surge by zone", r#"[{"zone":"Downtown","multiplier":{{ vars.surge }}},{"zone":"Airport","multiplier":1.8},{"zone":"Suburbs","multiplier":1.0}]"#),
            get("Surge", "/surge/zones/:zone", "Zone surge", r#"{"zone":"{{ request.params.zone }}","multiplier":{{ vars.surge }}}"#)
                .when("Unknown zone", 404, &[("param", "zone", "eq", "nowhere")], r#"{"error":"zone not found"}"#),
            get("Estimates", "/rates", "Rate card", r#"{"economy":{"per_km":1.2,"per_min":0.3},"comfort":{"per_km":1.8,"per_min":0.45},"xl":{"per_km":2.4,"per_min":0.6}}"#),
            get("Promos", "/promos/:code", "Promo code", r#"{"code":"{{ request.params.code }}","discount_percent":20}"#)
                .when("Expired promo", 410, &[("param", "code", "eq", "OLDPROMO")], r#"{"error":"promo expired"}"#),
            get("Promos", "/promos", "Active promos", r#"[{"code":"WELCOME20","discount_percent":20},{"code":"RIDE5","discount_percent":5}]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"pricing"}"#),
        ]);

    let payments = Srv::new("payments", "Payments", 4005)
        .folders(&["Charges", "Methods", "Payouts"])
        .ds("methods", 8, &["id:sequence", "brand:one_of:visa|mastercard|wallet|bank", "last4:regex:[0-9]{4}", "default:bool"])
        .routes(vec![
            crud("Methods", "/methods", "Payment methods", "methods"),
            post("Charges", "/charges", "Charge fare", 201, r#"{"id":"chg_{{ uuid() }}","trip":"{{ request.json.trip | default(value='unknown') }}","status":"succeeded","amount":{{ request.json.amount | default(value=0) }}}"#)
                .when("Card declined", 402, &[("body", "$.card", "eq", "4000000000000002")], r#"{"error":"card_declined"}"#)
                .when("Amount missing", 422, &[("body", "$.amount", "absent", "")], r#"{"error":"amount is required"}"#)
                .delay("150..400"),
            get("Charges", "/charges/:id", "Get charge", r#"{"id":"{{ request.params.id }}","status":"succeeded","amount":{{ random_int(min=8, max=70) }}.00}"#),
            post("Charges", "/charges/:id/refund", "Refund", 200, r#"{"charge":"{{ request.params.id }}","status":"refunded"}"#)
                .when("Already refunded", 409, &[("param", "id", "eq", "chg_refunded")], r#"{"error":"charge already refunded"}"#),
            post("Charges", "/tips", "Tip driver", 201, r#"{"id":"tip_{{ uuid() }}","amount":{{ request.json.amount | default(value=2) }}}"#)
                .when("Tip too large", 422, &[("body", "$.amount", "gt", "100")], r#"{"error":"tip must be at most 100"}"#),
            get("Payouts", "/payouts", "Driver payouts", &list(3, r#"{"id":"po_{{ uuid() }}","amount":{{ random_int(min=200, max=1200) }}.00,"paid_at":"{{ fake(kind="date.past") }}"}"#)),
            get("Payouts", "/payouts/:id", "Payout detail", r#"{"id":"{{ request.params.id }}","status":"paid"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"payments"}"#),
        ]);

    let ratings = Srv::new("ratings", "Ratings", 4006)
        .folders(&["Ratings", "Feedback"])
        .ds("ratings", 40, &["id:sequence", "trip_id:int:1:30", "stars:int:1:5", "comment:fake:lorem.sentence", "created:datetime"])
        .routes(vec![
            crud("Ratings", "/ratings", "Ratings", "ratings"),
            post("Ratings", "/trips/:id/rating", "Rate trip", 201, r#"{"trip":"{{ request.params.id }}","stars":{{ request.json.stars | default(value=5) }},"recorded":true}"#)
                .when("Stars out of range", 422, &[("body", "$.stars", "gt", "5")], r#"{"error":"stars must be 1 to 5"}"#)
                .when("Stars too low", 422, &[("body", "$.stars", "lt", "1")], r#"{"error":"stars must be 1 to 5"}"#),
            get("Ratings", "/drivers/:id/rating", "Driver rating", r#"{"driver_id":"{{ request.params.id }}","average":4.{{ random_int(min=2, max=9) }},"count":{{ random_int(min=20, max=400) }}}"#),
            get("Ratings", "/riders/:id/rating", "Rider rating", r#"{"rider_id":"{{ request.params.id }}","average":4.{{ random_int(min=2, max=9) }},"count":{{ random_int(min=5, max=200) }}}"#),
            get("Feedback", "/feedback/tags", "Feedback tags", r#"["clean car","friendly","safe driving","great route","late arrival"]"#),
            post("Feedback", "/feedback", "Submit feedback", 201, r#"{"id":"fb_{{ uuid() }}","comment":"{{ request.json.comment | default(value='') }}"}"#)
                .when("Comment too long", 413, &[("body", "$.long", "eq", "true")], r#"{"error":"comment too long"}"#),
            get("Feedback", "/feedback/recent", "Recent feedback", &list(3, r#"{"stars":{{ random_int(min=3, max=5) }},"comment":"{{ fake(kind="lorem.sentence") }}"}"#)).delay("20..80"),
            get("", "/health", "Health", r#"{"status":"ok","service":"ratings"}"#),
        ]);

    Ex {
        name: "ride-hailing",
        title: "Ride-hailing platform",
        story: "A ride-hailing backend: riders request trips, drivers go online and accept, the trips Server enforces a state machine (requested, accepted, arrived, in progress, completed), pricing applies a surge multiplier from a Server Variable, payments charge the fare and ratings close the loop.",
        servers: vec![riders, drivers, trips, pricing, payments, ratings],
        curls: vec![
            ("Fare estimate with surge (edit the surge Variable on the pricing Server)", "curl -s -X POST http://127.0.0.1:4004/estimate -H 'content-type: application/json' -d '{\"pickup\":\"Central Station\"}'".into()),
            ("Request a trip", "curl -s -X POST http://127.0.0.1:4003/trips/request -H 'content-type: application/json' -d '{\"pickup\":\"Central Station\"}'".into()),
            ("Completing an already completed trip returns 409", "curl -si -X POST http://127.0.0.1:4003/trips/3/complete".into()),
        ],
        files: vec![],
    }
}
