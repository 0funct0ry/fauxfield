use crate::exgen::*;

pub fn ex() -> Ex {
    let restaurants = Srv::new("restaurants", "Restaurants", 4001)
        .folders(&["Restaurants", "Reviews"])
        .ds("restaurants", 20, &["id:sequence", "name:one_of:Sunny Noodle Bar|Harbor Grill|Green Bowl|Pepper Pizzeria|Lotus Kitchen|Taco Corner|Maple Diner", "cuisine:one_of:noodles|grill|salad|pizza|indian|mexican|american", "rating:float:3:5", "open:bool", "city:fake:address.city"])
        .routes(vec![
            crud("Restaurants", "/restaurants", "Restaurants", "restaurants"),
            get("Restaurants", "/restaurants/search", "Search restaurants", &list(4, r#"{"id":{{ i + 1 }},"name":"{{ fake(kind="company.name") }}","cuisine":"{{ request.query.cuisine | default(value='any') }}","eta_min":{{ random_int(min=15, max=55) }}}"#))
                .when("Location required", 400, &[("query", "lat", "absent", "")], r#"{"error":"query parameter lat is required"}"#)
                .delay("40..140"),
            get("Restaurants", "/restaurants/nearby", "Nearby", &list(3, r#"{"id":{{ i + 1 }},"distance_km":{{ random_int(min=1, max=9) }}.2}"#)),
            get("Restaurants", "/restaurants/:id/hours", "Opening hours", r#"{"restaurant_id":"{{ request.params.id }}","open":"10:00","close":"22:30"}"#)
                .when("Closed", 200, &[("param", "id", "eq", "13")], r#"{"restaurant_id":13,"open":null,"close":null,"note":"closed today"}"#),
            get("Reviews", "/restaurants/:id/reviews", "Reviews", &list(3, r#"{"author":"{{ fake(kind="name.first") }}","stars":{{ random_int(min=3, max=5) }},"text":"{{ fake(kind="lorem.sentence") }}"}"#)),
            post("Reviews", "/restaurants/:id/reviews", "Post review", 201, r#"{"id":"rev_{{ uuid() }}","stars":{{ request.json.stars | default(value=5) }}}"#)
                .when("Invalid stars", 422, &[("body", "$.stars", "gt", "5")], r#"{"error":"stars must be 1 to 5"}"#),
            get("Restaurants", "/cuisines", "Cuisines", r#"["noodles","grill","salad","pizza","indian","mexican"]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"restaurants"}"#),
        ]);

    let menus = Srv::new("menus", "Menus", 4002)
        .folders(&["Menus", "Items"])
        .ds("items", 40, &["id:sequence", "restaurant_id:int:1:20", "name:one_of:Spicy Ramen|Margherita|Chicken Bowl|Veggie Wrap|Pad Thai|Burrito|Garden Salad|Fish Tacos", "price:float:4:30", "available:bool", "vegetarian:bool"])
        .routes(vec![
            crud("Items", "/items", "Menu items", "items"),
            get("Menus", "/restaurants/:id/menu", "Restaurant menu", &list(5, r#"{"id":{{ i + 1 }},"name":"{{ fake(kind="lorem.word") }} bowl","price":{{ random_int(min=5, max=28) }}.50}"#))
                .when("Menu unavailable", 404, &[("param", "id", "eq", "0")], r#"{"error":"menu not found"}"#)
                .delay("30..100"),
            get("Menus", "/restaurants/:id/categories", "Menu sections", r#"["starters","mains","desserts","drinks"]"#),
            get("Items", "/items/:id/options", "Item options", r#"{"item_id":"{{ request.params.id }}","options":[{"name":"Extra spicy","price":0.0},{"name":"Add egg","price":1.5}]}"#),
            get("Items", "/items/popular", "Popular items", &list(4, r#"{"id":{{ i + 1 }},"name":"{{ fake(kind="lorem.word") }}","orders":{{ random_int(min=50, max=900) }}}"#)),
            post("Items", "/items/:id/availability", "Set availability", 200, r#"{"item_id":"{{ request.params.id }}","available":{{ request.json.available | default(value=true) }}}"#)
                .when("Flag missing", 422, &[("body", "$.available", "absent", "")], r#"{"error":"available is required"}"#),
            get("Menus", "/allergens", "Allergens", r#"["gluten","dairy","nuts","soy","shellfish"]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"menus"}"#),
        ]);

    let orders = Srv::new("orders", "Orders", 4003)
        .folders(&["Orders", "Payments"])
        .ds("orders", 25, &["id:sequence", "customer:fake:name", "restaurant_id:int:1:20", "status:one_of:placed|preparing|picked_up|delivered|cancelled", "total:float:8:90", "placed_at:datetime"])
        .routes(vec![
            crud("Orders", "/orders", "Orders", "orders"),
            post("Orders", "/orders/place", "Place order", 201, r#"{"id":"ord_{{ uuid() }}","status":"placed","total":{{ request.json.total | default(value=0) }},"eta_min":{{ random_int(min=20, max=50) }}}"#)
                .when("Cart empty", 422, &[("body", "$.items", "absent", "")], r#"{"error":"items are required"}"#)
                .when("Restaurant closed", 409, &[("body", "$.restaurant_id", "eq", "13")], r#"{"error":"restaurant is closed"}"#)
                .delay("100..300"),
            post("Orders", "/orders/:id/cancel", "Cancel order", 200, r#"{"id":"{{ request.params.id }}","status":"cancelled"}"#)
                .when("Already picked up", 409, &[("param", "id", "lte", "5")], r#"{"error":"order already picked up"}"#),
            get("Orders", "/orders/:id/status", "Order status", r#"{"id":"{{ request.params.id }}","status":"preparing","updated":"{{ now() }}"}"#),
            get("Orders", "/orders/:id/receipt", "Receipt", r#"{"order":"{{ request.params.id }}","subtotal":24.5,"delivery_fee":2.99,"tip":3.0,"total":30.49}"#),
            post("Payments", "/orders/:id/pay", "Pay for order", 200, r#"{"order":"{{ request.params.id }}","payment":"pay_{{ uuid() }}","status":"captured"}"#)
                .when("Card declined", 402, &[("body", "$.card", "eq", "4000000000000002")], r#"{"error":"card declined"}"#)
                .delay("120..350"),
            get("Payments", "/payment-methods", "Payment methods", r#"["card","wallet","cash"]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"orders"}"#),
        ]);

    let couriers = Srv::new("couriers", "Couriers", 4004)
        .folders(&["Couriers", "Location"])
        .ds("couriers", 15, &["id:sequence", "name:fake:name", "phone:fake:phone", "vehicle:one_of:bike|scooter|car|walker", "status:one_of:available|busy|offline", "rating:float:3.5:5"])
        .routes(vec![
            crud("Couriers", "/couriers", "Couriers", "couriers"),
            get("Location", "/couriers/:id/location", "Courier location (poll me)", r#"{"courier_id":"{{ request.params.id }}","lat":40.{{ random_int(min=700000, max=799999) }},"lng":-73.{{ random_int(min=950000, max=999999) }},"heading":{{ random_int(min=0, max=359) }},"at":"{{ now() }}"}"#)
                .when("Courier offline", 404, &[("param", "id", "eq", "0")], r#"{"error":"courier is offline"}"#)
                .delay("20..60"),
            post("Location", "/couriers/:id/location", "Report location", 202, r#"{"courier_id":"{{ request.params.id }}","accepted":true}"#)
                .when("Coordinates missing", 422, &[("body", "$.lat", "absent", "")], r#"{"error":"lat and lng are required"}"#),
            post("Couriers", "/couriers/:id/accept", "Accept delivery", 200, r#"{"courier_id":"{{ request.params.id }}","order":"{{ request.json.order | default(value='ord_demo') }}","status":"busy"}"#)
                .when("Courier busy", 409, &[("param", "id", "gte", "90")], r#"{"error":"courier is already on a delivery"}"#),
            get("Couriers", "/couriers/available", "Available couriers", &list(3, r#"{"id":{{ i + 1 }},"name":"{{ fake(kind="name") }}","vehicle":"bike","distance_km":{{ random_int(min=1, max=5) }}.1}"#)),
            post("Couriers", "/couriers/:id/shift", "Start or end shift", 200, r#"{"courier_id":"{{ request.params.id }}","on_shift":true}"#),
            get("Couriers", "/couriers/:id/earnings", "Earnings", r#"{"courier_id":"{{ request.params.id }}","today":{{ random_int(min=20, max=180) }}.00,"deliveries":{{ random_int(min=2, max=16) }}}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"couriers"}"#),
        ]);

    let tracking = Srv::new("tracking", "Tracking", 4005)
        .folders(&["Tracking", "Streams"])
        .routes(vec![
            ws("Streams", "/stream", "Position stream", vec![
                ("connect", "", "", r#"{"type":"hello","message":"tracking connected"}"#.into()),
                ("interval", "2s", "", r#"{"type":"position","courier":"{{ fake(kind="name.first") }}","lat":40.{{ random_int(min=700000, max=799999) }},"lng":-73.{{ random_int(min=950000, max=999999) }},"eta_min":{{ random_int(min=1, max=30) }}}"#.into()),
                ("message", "", "ping", r#"{"type":"pong","at":"{{ now() }}"}"#.into()),
            ]),
            get("Tracking", "/orders/:id/tracking", "Tracking snapshot", r#"{"order":"{{ request.params.id }}","stage":"on_the_way","eta_min":{{ random_int(min=3, max=30) }},"courier":"{{ fake(kind="name") }}"}"#)
                .when("Unknown order", 404, &[("param", "id", "eq", "0")], r#"{"error":"order not found"}"#)
                .delay("20..80"),
            get("Tracking", "/orders/:id/timeline", "Delivery timeline", &list(4, r#"{"stage":"step {{ i + 1 }}","at":"{{ fake(kind="date.past") }}"}"#)),
            get("Tracking", "/orders/:id/eta", "ETA", r#"{"order":"{{ request.params.id }}","eta":"{{ fake(kind="date.future") }}"}"#),
            get("Tracking", "/orders/:id/route", "Route", r#"{"points":[{"lat":40.71,"lng":-73.99},{"lat":40.72,"lng":-73.98},{"lat":40.73,"lng":-73.97}]}"#),
            post("Tracking", "/orders/:id/share", "Share tracking link", 201, r#"{"url":"http://127.0.0.1:4005/track/{{ uuid() }}","expires_in":3600}"#)
                .when("Not yet dispatched", 409, &[("header", "x-stage", "eq", "preparing")], r#"{"error":"order has not been dispatched"}"#),
            post("Tracking", "/orders/:id/delivered", "Confirm delivery", 200, r#"{"order":"{{ request.params.id }}","delivered":true,"at":"{{ now() }}"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"tracking"}"#),
        ]);

    let promotions = Srv::new("promotions", "Promotions", 4006)
        .var("min_order", "15")
        .folders(&["Promotions", "Codes"])
        .ds("promotions", 8, &["id:sequence", "title:one_of:Free delivery|Two for one|Lunch deal|Weekend treat|First order|Late night", "discount_percent:int:5:40", "active:bool"])
        .routes(vec![
            crud("Promotions", "/promotions", "Promotions", "promotions"),
            post("Codes", "/codes/redeem", "Redeem code", 200, r#"{"code":"{{ request.json.code | default(value='') }}","discount_percent":20,"min_order":{{ vars.min_order }}}"#)
                .when("Expired code", 410, &[("body", "$.code", "eq", "OLD10")], r#"{"error":"code expired"}"#)
                .when("Unknown code", 404, &[("body", "$.code", "ne", "WELCOME20")], r#"{"error":"code not found"}"#)
                .delay("40..120"),
            get("Codes", "/codes/:code", "Code details", r#"{"code":"{{ request.params.code }}","percent":20,"valid_until":"{{ fake(kind="date.future") }}"}"#)
                .when("No such code", 404, &[("param", "code", "eq", "NOPE")], r#"{"error":"code not found"}"#),
            get("Promotions", "/promotions/active", "Active promotions", &list(3, r#"{"id":{{ i + 1 }},"title":"{{ fake(kind="lorem.word") }} special","discount_percent":{{ random_int(min=5, max=40) }}}"#)),
            get("Promotions", "/promotions/for/:restaurant", "Promotions for restaurant", &list(2, r#"{"restaurant":"{{ request.params.restaurant }}","title":"House special","discount_percent":10}"#)),
            post("Codes", "/codes/validate", "Validate order for code", 200, r#"{"valid":true,"saves":{{ random_int(min=1, max=12) }}.00}"#)
                .when("Below minimum", 422, &[("body", "$.total", "lt", "15")], r#"{"valid":false,"error":"order below minimum"}"#),
            get("Promotions", "/loyalty", "Loyalty points", r#"{"points":{{ random_int(min=0, max=900) }},"tier":"silver"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"promotions"}"#),
        ]);

    Ex {
        name: "food-delivery",
        title: "Food delivery",
        story: "Six services of a delivery app: restaurants, menus, orders, couriers (poll their location), a WebSocket tracking stream that pushes courier positions, and promotions with redeemable codes.",
        servers: vec![restaurants, menus, orders, couriers, tracking, promotions],
        curls: vec![
            ("List restaurants", "curl -s http://127.0.0.1:4001/restaurants".into()),
            ("Place an order", "curl -s -X POST http://127.0.0.1:4003/orders/place -H 'content-type: application/json' -d '{\"items\":[1,2],\"total\":24.5,\"restaurant_id\":3}'".into()),
            ("A closed restaurant returns 409", "curl -si -X POST http://127.0.0.1:4003/orders/place -H 'content-type: application/json' -d '{\"items\":[1],\"restaurant_id\":13}'".into()),
        ],
        files: vec![],
    }
}
