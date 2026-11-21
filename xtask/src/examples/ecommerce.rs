use crate::exgen::*;

pub fn ex() -> Ex {
    let catalog = Srv::new("catalog", "Catalog", 4001)
        .var("currency", "USD")
        .folders(&["Products", "Categories", "Inventory"])
        .ds("products", 40, &["id:sequence", "name:one_of:Trail Backpack|Steel Bottle|Desk Lamp|Canvas Tote|Wool Beanie|Camp Stove|Yoga Mat|Rain Jacket", "sku:regex:SKU-[0-9]{5}", "price:float:5:500", "category:one_of:outdoor|home|apparel|fitness", "stock:int:0:200", "active:bool"])
        .ds("categories", 6, &["id:sequence", "name:one_of:Outdoor|Home|Apparel|Fitness|Kitchen|Office", "slug:regex:[a-z]{6}"])
        .routes(vec![
            crud("Products", "/products", "Products", "products"),
            get("Products", "/search", "Search products", &list(5, r#"{"id":{{ i + 1 }},"name":"{{ fake(kind="lorem.word") }} {{ fake(kind="lorem.word") }}","price":{{ random_int(min=5, max=500) }}.99}"#))
                .when("Missing query", 400, &[("query", "q", "absent", "")], r#"{"error":"query parameter q is required"}"#)
                .delay("40..120"),
            get("Products", "/brands", "Brands", r#"["Northwind Gear","Contoso Home","Fabrikam Outdoors","Tailspin Wear"]"#),
            get("Categories", "/categories", "Categories", r#"[{"id":1,"name":"Outdoor"},{"id":2,"name":"Home"},{"id":3,"name":"Apparel"},{"id":4,"name":"Fitness"}]"#),
            get("Categories", "/categories/:id", "Get category", r#"{"id":"{{ request.params.id }}","name":"Category {{ request.params.id }}","product_count":{{ random_int(min=3, max=40) }}}"#)
                .when("Unknown category", 404, &[("param", "id", "eq", "999")], r#"{"error":"category not found"}"#),
            get("Inventory", "/inventory/:sku", "Stock level", r#"{"sku":"{{ request.params.sku }}","available":{{ random_int(min=0, max=120) }},"warehouse":"WH-{{ random_int(min=1, max=4) }}"}"#)
                .when("Out of stock", 200, &[("param", "sku", "eq", "SKU-00000")], r#"{"sku":"SKU-00000","available":0,"warehouse":"WH-1"}"#),
            post("Inventory", "/inventory/:sku/adjust", "Adjust stock", 200, r#"{"sku":"{{ request.params.sku }}","delta":{{ request.json.delta | default(value=0) }},"ok":true}"#)
                .when("Invalid delta", 422, &[("body", "$.delta", "absent", "")], r#"{"error":"delta is required"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"catalog"}"#),
        ]);

    let cart = Srv::new("cart", "Cart", 4002)
        .folders(&["Carts", "Coupons"])
        .ds("carts", 8, &["id:sequence", "user_id:int:1:20", "items:int:0:6", "total:float:10:900"])
        .routes(vec![
            crud("Carts", "/carts", "Carts", "carts"),
            get("Carts", "/carts/:id/items", "Cart items", &list(3, r#"{"product_id":{{ i + 1 }},"qty":{{ random_int(min=1, max=3) }},"price":{{ random_int(min=5, max=99) }}.00}"#)),
            post("Carts", "/carts/:id/items", "Add item", 201, r#"{"cart_id":"{{ request.params.id }}","product_id":{{ request.json.product_id | default(value=1) }},"qty":{{ request.json.qty | default(value=1) }}}"#)
                .when("Quantity too large", 422, &[("body", "$.qty", "gt", "99")], r#"{"error":"qty must be at most 99"}"#),
            route("Carts", "DELETE", "/carts/:id/items/:product_id", "Remove item", r#"{"removed":"{{ request.params.product_id }}"}"#),
            post("Coupons", "/carts/:id/coupon", "Apply coupon", 200, r#"{"code":"{{ request.json.code | default(value='') }}","discount_percent":15}"#)
                .when("Expired coupon", 410, &[("body", "$.code", "eq", "EXPIRED10")], r#"{"error":"coupon expired"}"#)
                .when("Unknown coupon", 404, &[("body", "$.code", "ne", "SAVE15")], r#"{"error":"coupon not found"}"#),
            get("Coupons", "/coupons", "List coupons", r#"[{"code":"SAVE15","percent":15},{"code":"FREESHIP","percent":0}]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"cart"}"#),
        ]);

    let orders = Srv::new("orders", "Orders", 4003)
        .folders(&["Orders", "Webhooks"])
        .ds("orders", 25, &["id:sequence", "customer:fake:name", "status:one_of:pending|paid|shipped|delivered|cancelled", "total:float:10:900", "placed_at:datetime"])
        .routes(vec![
            crud("Orders", "/orders", "Orders", "orders"),
            post("Orders", "/orders/:id/cancel", "Cancel order", 200, r#"{"id":"{{ request.params.id }}","status":"cancelled"}"#)
                .when("Already shipped", 409, &[("param", "id", "lte", "10")], r#"{"error":"order already shipped"}"#),
            post("Orders", "/orders/:id/status", "Transition status", 200, r#"{"id":"{{ request.params.id }}","status":"{{ request.json.status | default(value='') }}"}"#)
                .when("Missing status", 422, &[("body", "$.status", "absent", "")], r#"{"error":"status is required"}"#),
            get("Orders", "/orders/:id/invoice", "Invoice", r#"{"invoice":"INV-{{ request.params.id }}","lines":[{"sku":"SKU-10001","qty":2,"amount":39.98}],"total":39.98,"currency":"USD"}"#),
            get("Orders", "/orders/:id/timeline", "Timeline", r#"[{"status":"pending","at":"2025-01-05T10:00:00Z"},{"status":"paid","at":"2025-01-05T10:02:00Z"}]"#),
            post("Webhooks", "/webhooks/payments", "Payment webhook receiver", 200, r#"{"received":true,"event":"{{ request.json.event | default(value='unknown') }}"}"#),
            get("Orders", "/orders/stats", "Order stats", r#"{"pending":4,"paid":9,"shipped":7,"delivered":5}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"orders"}"#),
        ]);

    let payments = Srv::new("payments", "Payments", 4004)
        .folders(&["Charges", "Refunds"])
        .routes(vec![
            post("Charges", "/charges", "Create charge", 201, r#"{"id":"ch_{{ uuid() }}","status":"succeeded","amount":{{ request.json.amount | default(value=0) }},"currency":"usd"}"#)
                .when("Card declined", 402, &[("body", "$.card", "eq", "4000000000000002")], r#"{"error":{"code":"card_declined","message":"Your card was declined."}}"#)
                .delay("150..400")
                .cb(Cb { name: "Notify orders", url: "{{ vars.base_url_orders }}/webhooks/payments".into(), delay_ms: 500, retries: 2, body: r#"{"event":"payment.succeeded","amount":{{ request.json.amount | default(value=0) }}}"#.into() }),
            get("Charges", "/charges/:id", "Get charge", r#"{"id":"{{ request.params.id }}","status":"succeeded","amount":4999}"#),
            get("Charges", "/charges", "List charges", &list(4, r#"{"id":"ch_{{ uuid() }}","amount":{{ random_int(min=500, max=9999) }},"status":"succeeded"}"#)),
            post("Refunds", "/refunds", "Create refund", 201, r#"{"id":"re_{{ uuid() }}","charge":"{{ request.json.charge | default(value='') }}","status":"succeeded"}"#)
                .when("Charge not found", 404, &[("body", "$.charge", "absent", "")], r#"{"error":"charge is required"}"#),
            get("Refunds", "/refunds/:id", "Get refund", r#"{"id":"{{ request.params.id }}","status":"succeeded"}"#),
            get("Charges", "/methods", "Payment methods", r#"["card","bank_transfer","wallet"]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"payments"}"#),
        ]);

    let shipping = Srv::new("shipping", "Shipping", 4005)
        .folders(&["Rates", "Tracking", "Carriers"])
        .ds("carriers", 5, &["id:sequence", "name:one_of:Swiftline|Roadrunner Freight|Harbor Post|SkyParcel|GreenMile", "days:int:1:7"])
        .routes(vec![
            crud("Carriers", "/carriers", "Carriers", "carriers"),
            post("Rates", "/rates", "Quote rates", 200, r#"[{"carrier":"Swiftline","days":2,"price":12.5},{"carrier":"Harbor Post","days":5,"price":6.9}]"#)
                .when("Missing postcode", 400, &[("body", "$.postcode", "absent", "")], r#"{"error":"postcode is required"}"#)
                .delay("80..250"),
            get("Rates", "/rates/zones", "Zones", r#"["domestic","regional","international"]"#),
            get("Tracking", "/tracking/:code", "Track parcel", r#"{"code":"{{ request.params.code }}","status":"in_transit","events":[{"at":"2025-01-06T08:00:00Z","note":"Departed hub"}]}"#)
                .when("Unknown parcel", 404, &[("param", "code", "eq", "UNKNOWN")], r#"{"error":"parcel not found"}"#),
            get("Tracking", "/tracking/:code/events", "Parcel events", &list(3, r#"{"at":"{{ fake(kind="date.past") }}","note":"Scan {{ i + 1 }}"}"#)),
            post("Tracking", "/labels", "Create label", 201, r#"{"label_id":"lbl_{{ uuid() }}","url":"http://127.0.0.1:4005/labels/pdf"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"shipping"}"#),
        ]);

    let users = Srv::new("users", "Users", 4006)
        .folders(&["Accounts", "Addresses"])
        .ds("users", 30, &["id:sequence", "name:fake:name", "email:fake:internet.email", "phone:fake:phone", "joined:datetime"])
        .ds("addresses", 40, &["id:sequence", "user_id:int:1:30", "street:fake:address.street", "city:fake:address.city", "postcode:fake:address.postcode"])
        .routes(vec![
            post("Accounts", "/register", "Register", 201, r#"{"id":{{ random_int(min=100, max=999) }},"email":"{{ request.json.email | default(value='') }}"}"#)
                .when("Email missing", 422, &[("body", "$.email", "absent", "")], r#"{"error":"email is required"}"#)
                .when("Email taken", 409, &[("body", "$.email", "eq", "taken@example.test")], r#"{"error":"email already registered"}"#),
            post("Accounts", "/login", "Login", 200, r#"{"token":"tok_{{ uuid() }}","expires_in":3600}"#)
                .when("Bad credentials", 401, &[("body", "$.password", "ne", "demo")], r#"{"error":"invalid credentials"}"#),
            get("Accounts", "/profile", "Profile", r#"{"id":1,"name":"{{ fake(kind="name") }}","email":"{{ fake(kind="internet.email") }}"}"#)
                .when("Unauthorized", 401, &[("header", "authorization", "absent", "")], r#"{"error":"missing token"}"#),
            crud("Accounts", "/users", "Users", "users"),
            crud("Addresses", "/addresses", "Addresses", "addresses"),
            get("Addresses", "/users/:id/addresses", "User addresses", &list(2, r#"{"street":"{{ fake(kind="address.street") }}","city":"{{ fake(kind="address.city") }}"}"#)),
            get("", "/health", "Health", r#"{"status":"ok","service":"users"}"#),
        ]);

    Ex {
        name: "ecommerce",
        title: "E-commerce platform",
        story: "Six microservices of an online shop: browse the catalog, fill a cart, place an order, pay (the payments Server fires a webhook Callback to the orders Server), ship and manage users.",
        servers: vec![catalog, cart, orders, payments, shipping, users],
        curls: vec![
            ("List products", "curl -s http://127.0.0.1:4001/products".into()),
            ("Charge a card (watch the webhook Callback in Traffic)", "curl -s -X POST http://127.0.0.1:4004/charges -H 'content-type: application/json' -d '{\"amount\":4999,\"card\":\"4242424242424242\"}'".into()),
            ("A declined card returns 402", "curl -si -X POST http://127.0.0.1:4004/charges -H 'content-type: application/json' -d '{\"amount\":4999,\"card\":\"4000000000000002\"}'".into()),
        ],
        files: vec![],
    }
}
