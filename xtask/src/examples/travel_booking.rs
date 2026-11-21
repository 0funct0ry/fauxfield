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
    let flights = Srv::new("flights", "Flights", 4001)
        .var("currency", "EUR")
        .folders(&["Search", "Fares", "Airports"])
        .ds("flights", 30, &["id:sequence", "number:regex:FX[0-9]{3}", "origin:one_of:BER|LIS|OSL|VIE|ATH|DUB", "destination:one_of:AMS|PRG|CPH|MAD|ROM|ZRH", "departs:datetime", "seats:int:0:180", "price:float:49:620"])
        .routes(vec![
            crud("Search", "/flights", "Flights", "flights"),
            get("Search", "/flights/search", "Search flights", &list(5, r#"{"id":{{ i + 1 }},"number":"FX{{ random_int(min=100, max=999) }}","from":"{{ request.query.from | default(value='BER') }}","to":"{{ request.query.to | default(value='AMS') }}","price":{{ random_int(min=49, max=620) }}.00,"currency":"{{ vars.currency }}"}"#))
                .when("Missing origin", 400, &[("query", "from", "absent", "")], r#"{"error":"query parameter from is required"}"#)
                .delay("120..450"),
            get("Fares", "/flights/:id/fares", "Fares", r#"[{"class":"basic","price":89.0},{"class":"flex","price":139.0},{"class":"business","price":420.0}]"#)
                .when("Flight not found", 404, &[("param", "id", "eq", "999")], r#"{"error":"flight not found"}"#),
            get("Fares", "/flights/:id/seats", "Seat map", &list(6, r#"{"seat":"{{ i + 1 }}A","free":true}"#)),
            get("Fares", "/fares/calendar", "Fare calendar", &list(7, r#"{"day":{{ i + 1 }},"lowest":{{ random_int(min=49, max=300) }}}"#)),
            get("Airports", "/airports", "Airports", r#"[{"code":"BER","city":"Berlin"},{"code":"LIS","city":"Lisbon"},{"code":"AMS","city":"Amsterdam"},{"code":"PRG","city":"Prague"}]"#),
            get("Airports", "/airports/:code", "Airport", r#"{"code":"{{ request.params.code }}","city":"{{ fake(kind="address.city") }}","country":"{{ fake(kind="address.country") }}"}"#),
            get("Search", "/flights/:id/status", "Flight status", r#"{"id":"{{ request.params.id }}","status":"on_time","gate":"B{{ random_int(min=1, max=40) }}"}"#),
            health("flights"),
        ]);

    let hotels = Srv::new("hotels", "Hotels", 4002)
        .folders(&["Properties", "Availability", "Reviews"])
        .ds("hotels", 20, &["id:sequence", "name:one_of:Harbor Inn|Maple Court|Grand Linden|Seaside Rest|Old Town Lodge|Skyline Suites", "city:fake:address.city", "stars:int:2:5", "price_night:float:60:420"])
        .routes(vec![
            crud("Properties", "/hotels", "Hotels", "hotels"),
            get("Properties", "/hotels/search", "Search hotels", &list(4, r#"{"id":{{ i + 1 }},"name":"{{ fake(kind="company.name") }} Hotel","city":"{{ request.query.city | default(value='Lisbon') }}","price_night":{{ random_int(min=60, max=420) }}.00}"#))
                .when("City missing", 400, &[("query", "city", "absent", "")], r#"{"error":"query parameter city is required"}"#)
                .delay("100..350"),
            get("Properties", "/hotels/:id/rooms", "Rooms", r#"[{"type":"double","price":120.0},{"type":"suite","price":260.0}]"#)
                .when("Hotel not found", 404, &[("param", "id", "eq", "999")], r#"{"error":"hotel not found"}"#),
            get("Availability", "/hotels/:id/availability", "Availability", r#"{"hotel":"{{ request.params.id }}","available":true,"rooms_left":{{ random_int(min=0, max=9) }}}"#)
                .when("Sold out", 200, &[("param", "id", "eq", "13")], r#"{"hotel":13,"available":false,"rooms_left":0}"#),
            post("Availability", "/hotels/:id/quote", "Quote stay", 200, r#"{"hotel":"{{ request.params.id }}","nights":{{ request.json.nights | default(value=1) }},"total":{{ random_int(min=100, max=2000) }}.00}"#)
                .when("Too many nights", 422, &[("body", "$.nights", "gt", "30")], r#"{"error":"stays are limited to 30 nights"}"#),
            get("Reviews", "/hotels/:id/reviews", "Reviews", &list(3, r#"{"author":"{{ fake(kind="internet.username") }}","stars":{{ random_int(min=3, max=5) }},"text":"{{ fake(kind="lorem.sentence") }}"}"#)),
            post("Reviews", "/hotels/:id/reviews", "Add review", 201, r#"{"id":"rv_{{ uuid() }}","stars":{{ request.json.stars | default(value=0) }}}"#)
                .when("Stars missing", 422, &[("body", "$.stars", "absent", "")], r#"{"error":"stars is required"}"#),
            get("Properties", "/destinations", "Destinations", r#"["Lisbon","Prague","Vienna","Athens"]"#),
            health("hotels"),
        ]);

    let cars = Srv::new("cars", "Cars", 4003)
        .folders(&["Fleet", "Rentals"])
        .ds("cars", 18, &["id:sequence", "model:one_of:Compact|Estate|SUV|Convertible|Electric|Minivan", "plate:regex:[A-Z]{2}-[0-9]{4}", "daily_rate:float:25:180", "available:bool"])
        .routes(vec![
            crud("Fleet", "/cars", "Cars", "cars"),
            get("Fleet", "/cars/search", "Search cars", &list(4, r#"{"id":{{ i + 1 }},"model":"Compact","daily_rate":{{ random_int(min=25, max=180) }}.00,"pickup":"{{ request.query.pickup | default(value='BER') }}"}"#))
                .when("Pickup missing", 400, &[("query", "pickup", "absent", "")], r#"{"error":"query parameter pickup is required"}"#)
                .delay("80..250"),
            get("Fleet", "/cars/:id/extras", "Extras", r#"["child_seat","gps","extra_driver","full_insurance"]"#),
            post("Rentals", "/rentals", "Create rental", 201, r#"{"id":"rn_{{ uuid() }}","car":{{ request.json.car_id | default(value=0) }},"status":"reserved"}"#)
                .when("Car missing", 422, &[("body", "$.car_id", "absent", "")], r#"{"error":"car_id is required"}"#)
                .when("Car unavailable", 409, &[("body", "$.car_id", "eq", "3")], r#"{"error":"car is not available"}"#),
            get("Rentals", "/rentals/:id", "Get rental", r#"{"id":"{{ request.params.id }}","status":"reserved","driver":"{{ fake(kind="name") }}"}"#)
                .when("Rental not found", 404, &[("param", "id", "eq", "missing")], r#"{"error":"rental not found"}"#),
            route("Rentals", "DELETE", "/rentals/:id", "Cancel rental", r#"{"id":"{{ request.params.id }}","status":"cancelled"}"#),
            get("Fleet", "/locations", "Pickup locations", r#"[{"code":"BER","name":"Berlin Airport"},{"code":"LIS","name":"Lisbon Central"},{"code":"VIE","name":"Vienna Station"}]"#),
            health("cars"),
        ]);

    let bookings = Srv::new("bookings", "Bookings", 4004)
        .var("hold_minutes", "15")
        .folders(&["Bookings", "Webhooks"])
        .ds("bookings", 20, &["id:sequence", "traveler:fake:name", "email:fake:internet.email", "status:one_of:held|confirmed|cancelled", "total:float:80:2400", "created:datetime"])
        .routes(vec![
            crud("Bookings", "/bookings", "Bookings", "bookings"),
            post("Bookings", "/bookings/hold", "Hold booking", 201, r#"{"id":"bk_{{ uuid() }}","status":"held","expires_in_minutes":{{ vars.hold_minutes }},"item":"{{ request.json.item | default(value='flight') }}"}"#)
                .when("Item missing", 422, &[("body", "$.item", "absent", "")], r#"{"error":"item is required"}"#)
                .delay("100..300"),
            post("Bookings", "/bookings/:id/confirm", "Confirm booking", 200, r#"{"id":"{{ request.params.id }}","status":"confirmed","reference":"{{ fake(kind="finance.bic") }}"}"#)
                .when("Hold expired", 410, &[("param", "id", "eq", "expired")], r#"{"error":"hold expired"}"#)
                .when("Payment required", 402, &[("header", "x-payment-token", "absent", "")], r#"{"error":"payment token is required"}"#),
            post("Bookings", "/bookings/:id/cancel", "Cancel booking", 200, r#"{"id":"{{ request.params.id }}","status":"cancelled","refund":true}"#)
                .when("Already cancelled", 409, &[("param", "id", "eq", "cancelled")], r#"{"error":"booking already cancelled"}"#),
            get("Bookings", "/bookings/:id/itinerary", "Itinerary", r#"{"id":"{{ request.params.id }}","segments":[{"type":"flight","number":"FX123"},{"type":"hotel","name":"Harbor Inn"}]}"#),
            get("Bookings", "/bookings/:id/voucher", "Voucher", r#"{"id":"{{ request.params.id }}","code":"{{ fake(kind="color.hex") }}","valid":true}"#),
            post("Webhooks", "/webhooks/payments", "Payment webhook receiver", 200, r#"{"received":true,"event":"{{ request.json.event | default(value='unknown') }}"}"#),
            get("Bookings", "/bookings/stats", "Stats", r#"{"held":4,"confirmed":11,"cancelled":3}"#),
            health("bookings"),
        ]);

    let payments = Srv::new("payments", "Payments", 4005)
        .folders(&["Payments", "Refunds"])
        .routes(vec![
            post("Payments", "/payments", "Create payment", 201, r#"{"id":"pay_{{ uuid() }}","status":"succeeded","amount":{{ request.json.amount | default(value=0) }},"currency":"eur","booking":"{{ request.json.booking | default(value='') }}"}"#)
                .when("Card declined", 402, &[("body", "$.card", "eq", "4000000000000002")], r#"{"error":{"code":"card_declined","message":"The card was declined."}}"#)
                .when("Amount missing", 422, &[("body", "$.amount", "absent", "")], r#"{"error":"amount is required"}"#)
                .delay("150..450")
                .cb(Cb { name: "Notify bookings", url: "{{ vars.base_url_bookings }}/webhooks/payments".into(), delay_ms: 400, retries: 2, body: r#"{"event":"payment.succeeded","amount":{{ request.json.amount | default(value=0) }}}"#.into() }),
            get("Payments", "/payments/:id", "Get payment", r#"{"id":"{{ request.params.id }}","status":"succeeded","amount":24900}"#)
                .when("Payment not found", 404, &[("param", "id", "eq", "missing")], r#"{"error":"payment not found"}"#),
            get("Payments", "/payments", "List payments", &list(4, r#"{"id":"pay_{{ uuid() }}","amount":{{ random_int(min=5000, max=90000) }},"status":"succeeded"}"#)),
            post("Refunds", "/refunds", "Create refund", 201, r#"{"id":"re_{{ uuid() }}","payment":"{{ request.json.payment | default(value='') }}","status":"succeeded"}"#)
                .when("Payment missing", 422, &[("body", "$.payment", "absent", "")], r#"{"error":"payment is required"}"#),
            get("Refunds", "/refunds/:id", "Get refund", r#"{"id":"{{ request.params.id }}","status":"succeeded"}"#),
            get("Payments", "/methods", "Payment methods", r#"["card","bank_transfer","wallet"]"#),
            get("Payments", "/receipts/:id", "Receipt", r#"{"payment":"{{ request.params.id }}","iban":"{{ fake(kind="finance.iban") }}","total":249.0}"#),
            health("payments"),
        ]);

    let loyalty = Srv::new("loyalty", "Loyalty", 4006)
        .folders(&["Members", "Rewards"])
        .ds("members", 20, &["id:sequence", "name:fake:name", "tier:one_of:bronze|silver|gold|platinum", "points:int:0:90000", "joined:date"])
        .ds("rewards", 6, &["id:sequence", "name:one_of:Lounge Pass|Free Night|Seat Upgrade|Car Day|Extra Bag|Priority Boarding", "cost:int:2000:40000"])
        .routes(vec![
            crud("Members", "/members", "Members", "members"),
            crud("Rewards", "/rewards", "Rewards", "rewards"),
            get("Members", "/members/:id/balance", "Balance", r#"{"member":"{{ request.params.id }}","points":{{ random_int(min=0, max=90000) }},"tier":"silver"}"#)
                .when("Member not found", 404, &[("param", "id", "eq", "999")], r#"{"error":"member not found"}"#),
            post("Members", "/members/:id/earn", "Earn points", 200, r#"{"member":"{{ request.params.id }}","earned":{{ request.json.points | default(value=0) }}}"#)
                .when("Points invalid", 422, &[("body", "$.points", "lte", "0")], r#"{"error":"points must be positive"}"#),
            post("Rewards", "/members/:id/redeem", "Redeem reward", 200, r#"{"member":"{{ request.params.id }}","reward":{{ request.json.reward_id | default(value=1) }},"voucher":"{{ uuid() }}"}"#)
                .when("Not enough points", 402, &[("body", "$.reward_id", "eq", "6")], r#"{"error":"not enough points"}"#)
                .delay("60..200"),
            get("Members", "/members/:id/history", "History", &list(4, r#"{"at":"{{ fake(kind="date.past") }}","delta":{{ random_int(min=100, max=2500) }},"note":"{{ fake(kind="lorem.word") }}"}"#)),
            get("Members", "/tiers", "Tiers", r#"[{"tier":"bronze","from":0},{"tier":"silver","from":10000},{"tier":"gold","from":30000},{"tier":"platinum","from":60000}]"#),
            health("loyalty"),
        ]);

    Ex {
        name: "travel-booking",
        title: "Travel booking",
        story: "A travel agency backend: search flights, hotels and cars, hold then confirm or cancel a booking, pay (the payments Server fires a webhook Callback to the bookings Server) and earn loyalty points.",
        servers: vec![flights, hotels, cars, bookings, payments, loyalty],
        curls: vec![
            ("Search flights", "curl -s 'http://127.0.0.1:4001/flights/search?from=BER&to=AMS'".into()),
            ("Hold a booking", "curl -s -X POST http://127.0.0.1:4004/bookings/hold -H 'content-type: application/json' -d '{\"item\":\"flight\"}'".into()),
            ("Pay (watch the webhook Callback in Traffic); the card 4000000000000002 returns 402", "curl -s -X POST http://127.0.0.1:4005/payments -H 'content-type: application/json' -d '{\"amount\":24900,\"card\":\"4242424242424242\"}'".into()),
        ],
        files: vec![],
    }
}
