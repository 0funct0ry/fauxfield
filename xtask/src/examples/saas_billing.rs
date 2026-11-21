use crate::exgen::*;

pub fn ex() -> Ex {
    let auth = Srv::new("auth", "Auth", 4001)
        .folders(&["OAuth", "Sessions", "Keys"])
        .ds("api_keys", 8, &["id:sequence", "label:fake:lorem.word", "prefix:regex:sk_[a-z0-9]{8}", "active:bool"])
        .routes(vec![
            post("OAuth", "/oauth/token", "Token endpoint", 200, r#"{"access_token":"at_{{ uuid() }}","refresh_token":"rt_{{ uuid() }}","token_type":"Bearer","expires_in":3600}"#)
                .when("Missing grant_type", 400, &[("body", "$.grant_type", "absent", "")], r#"{"error":"invalid_request","error_description":"grant_type is required"}"#)
                .when("Bad client secret", 401, &[("body", "$.client_secret", "eq", "wrong")], r#"{"error":"invalid_client"}"#)
                .when("Refresh grant", 200, &[("body", "$.grant_type", "eq", "refresh_token")], r#"{"access_token":"at_{{ uuid() }}","token_type":"Bearer","expires_in":3600}"#)
                .delay("60..200"),
            post("OAuth", "/oauth/revoke", "Revoke token", 200, r#"{"revoked":true}"#),
            get("OAuth", "/oauth/jwks", "JWKS", r#"{"keys":[{"kty":"RSA","kid":"demo-key-1","use":"sig","alg":"RS256"}]}"#),
            get("OAuth", "/.well-known/openid-configuration", "OIDC discovery", r#"{"issuer":"http://127.0.0.1:4001","token_endpoint":"http://127.0.0.1:4001/oauth/token","jwks_uri":"http://127.0.0.1:4001/oauth/jwks"}"#),
            get("Sessions", "/me", "Current user", r#"{"id":"usr_{{ uuid() }}","name":"{{ fake(kind="name") }}","email":"{{ fake(kind="internet.email") }}","role":"member"}"#)
                .when("Not authenticated", 401, &[("header", "authorization", "absent", "")], r#"{"error":"unauthorized"}"#)
                .when("Token expired", 401, &[("header", "authorization", "eq", "Bearer expired")], r#"{"error":"token_expired"}"#),
            get("Sessions", "/admin/users", "Admin user list", &list(4, r#"{"id":{{ i + 1 }},"email":"{{ fake(kind="internet.email") }}","role":"member"}"#))
                .when("Not authenticated", 401, &[("header", "authorization", "absent", "")], r#"{"error":"unauthorized"}"#)
                .when("Not an admin", 403, &[("header", "x-role", "ne", "admin")], r#"{"error":"forbidden","message":"admin role required"}"#),
            crud("Keys", "/api-keys", "API keys", "api_keys"),
            post("Sessions", "/logout", "Logout", 204, ""),
            get("", "/health", "Health", r#"{"status":"ok","service":"auth"}"#),
        ]);

    let tenants = Srv::new("tenants", "Tenants", 4002)
        .folders(&["Tenants", "Members", "Hooks"])
        .ds("tenants", 20, &["id:sequence", "name:fake:company.name", "plan:one_of:free|team|business|enterprise", "region:one_of:us|eu|apac", "created:datetime"])
        .ds("members", 40, &["id:sequence", "tenant_id:int:1:20", "email:fake:internet.email", "role:one_of:owner|admin|member"])
        .routes(vec![
            crud("Tenants", "/tenants", "Tenants", "tenants"),
            crud("Members", "/members", "Members", "members"),
            get("Tenants", "/tenants/:id/settings", "Tenant settings", r#"{"tenant_id":"{{ request.params.id }}","sso":false,"data_region":"us","seats":{{ random_int(min=5, max=200) }}}"#)
                .when("Tenant suspended", 403, &[("param", "id", "eq", "666")], r#"{"error":"tenant suspended"}"#),
            get("Tenants", "/tenants/:id/limits", "Plan limits", r#"{"tenant_id":"{{ request.params.id }}","seats":50,"api_calls":100000,"storage_gb":100}"#),
            post("Tenants", "/tenants/:id/suspend", "Suspend", 200, r#"{"tenant_id":"{{ request.params.id }}","status":"suspended"}"#)
                .when("Not an admin", 403, &[("header", "x-role", "eq", "member")], r#"{"error":"forbidden"}"#),
            post("Members", "/tenants/:id/invites", "Invite member", 201, r#"{"invite":"inv_{{ uuid() }}","email":"{{ request.json.email | default(value='guest@example.test') }}"}"#)
                .when("Email missing", 422, &[("body", "$.email", "absent", "")], r#"{"error":"email is required"}"#),
            post("Hooks", "/hooks/inbound", "Inbound webhook receiver", 200, r#"{"received":true,"event":"{{ request.json.type | default(value='unknown') }}"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"tenants"}"#),
        ]);

    let subscriptions = Srv::new("subscriptions", "Subscriptions", 4003)
        .folders(&["Subscriptions", "Plans"])
        .ds("subscriptions", 20, &["id:sequence", "tenant_id:int:1:20", "plan:one_of:free|team|business|enterprise", "status:one_of:active|trialing|past_due|cancelled", "seats:int:1:200"])
        .routes(vec![
            crud("Subscriptions", "/subscriptions", "Subscriptions", "subscriptions"),
            get("Plans", "/plans", "Plans", r#"[{"id":"free","price":0},{"id":"team","price":29},{"id":"business","price":99},{"id":"enterprise","price":499}]"#),
            get("Plans", "/plans/:id", "Plan detail", r#"{"id":"{{ request.params.id }}","interval":"month","currency":"USD","features":["sso","audit_log"]}"#)
                .when("Unknown plan", 404, &[("param", "id", "eq", "platinum")], r#"{"error":"plan not found"}"#),
            post("Subscriptions", "/subscriptions/start", "Start subscription", 201, r#"{"id":"sub_{{ uuid() }}","plan":"{{ request.json.plan | default(value='team') }}","status":"trialing","trial_days":14}"#)
                .when("Plan missing", 422, &[("body", "$.plan", "absent", "")], r#"{"error":"plan is required"}"#)
                .when("Payment required", 402, &[("body", "$.card", "eq", "4000000000000002")], r#"{"error":"payment_required"}"#)
                .delay("100..300")
                .cb(Cb { name: "Notify webhooks", url: "{{ vars.base_url_webhooks }}/events".into(), delay_ms: 300, retries: 3, body: r#"{"type":"subscription.created","plan":"{{ request.json.plan | default(value='team') }}"}"#.into() }),
            post("Subscriptions", "/subscriptions/:id/upgrade", "Upgrade", 200, r#"{"id":"{{ request.params.id }}","plan":"{{ request.json.plan | default(value='business') }}","prorated":true}"#)
                .when("Already on plan", 409, &[("body", "$.plan", "eq", "current")], r#"{"error":"already on this plan"}"#),
            post("Subscriptions", "/subscriptions/:id/cancel", "Cancel", 200, r#"{"id":"{{ request.params.id }}","status":"cancelled","ends":"{{ fake(kind="date.future") }}"}"#)
                .when("Already cancelled", 409, &[("param", "id", "eq", "cancelled")], r#"{"error":"already cancelled"}"#),
            get("Subscriptions", "/subscriptions/:id/preview", "Proration preview", r#"{"id":"{{ request.params.id }}","amount_due":{{ random_int(min=5, max=300) }}.00,"currency":"USD"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"subscriptions"}"#),
        ]);

    let invoices = Srv::new("invoices", "Invoices", 4004)
        .folders(&["Invoices", "Payments"])
        .ds("invoices", 30, &["id:sequence", "tenant_id:int:1:20", "number:regex:INV-[0-9]{6}", "amount:float:29:999", "status:one_of:draft|open|paid|void|uncollectible", "issued:datetime"])
        .routes(vec![
            crud("Invoices", "/invoices", "Invoices", "invoices"),
            get("Invoices", "/invoices/:id/lines", "Invoice lines", &list(3, r#"{"description":"{{ fake(kind="lorem.sentence") }}","qty":{{ random_int(min=1, max=10) }},"amount":{{ random_int(min=5, max=200) }}.00}"#)),
            get("Invoices", "/invoices/:id/pdf", "Invoice document", r#"{"url":"http://127.0.0.1:4004/files/{{ request.params.id }}.pdf","expires_in":300}"#)
                .when("Invoice missing", 404, &[("param", "id", "eq", "999")], r#"{"error":"invoice not found"}"#),
            post("Payments", "/invoices/:id/pay", "Pay invoice", 200, r#"{"id":"{{ request.params.id }}","status":"paid","receipt":"rcp_{{ uuid() }}"}"#)
                .when("Already paid", 409, &[("param", "id", "lte", "3")], r#"{"error":"invoice already paid"}"#)
                .when("Card declined", 402, &[("body", "$.card", "eq", "4000000000000002")], r#"{"error":"card_declined"}"#)
                .delay("200..500"),
            post("Invoices", "/invoices/:id/void", "Void invoice", 200, r#"{"id":"{{ request.params.id }}","status":"void"}"#),
            get("Invoices", "/invoices/upcoming", "Upcoming invoice", r#"{"amount_due":{{ random_int(min=29, max=500) }}.00,"due":"{{ fake(kind="date.future") }}"}"#),
            get("Payments", "/payment-methods", "Payment methods", r#"[{"id":"pm_1","brand":"visa","last4":"4242"},{"id":"pm_2","brand":"mastercard","last4":"1111"}]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"invoices"}"#),
        ]);

    let usage = Srv::new("usage", "Usage", 4005)
        .var("overage_rate", "0.002")
        .folders(&["Events", "Reports"])
        .ds("meters", 6, &["id:sequence", "name:one_of:api_calls|storage_gb|seats|emails|builds|bandwidth_gb", "unit:one_of:call|gb|seat|email|build|gb", "included:int:100:100000"])
        .routes(vec![
            crud("Reports", "/meters", "Meters", "meters"),
            post("Events", "/usage/events", "Record usage", 202, r#"{"accepted":true,"event":"evt_{{ uuid() }}","quantity":{{ request.json.quantity | default(value=1) }}}"#)
                .when("Quantity missing", 422, &[("body", "$.quantity", "absent", "")], r#"{"error":"quantity is required"}"#)
                .when("Quota exceeded", 429, &[("body", "$.quantity", "gt", "100000")], r#"{"error":"rate_limited","retry_after":30}"#),
            get("Reports", "/usage/summary", "Usage summary", r#"{"period":"current","api_calls":{{ random_int(min=1000, max=90000) }},"storage_gb":{{ random_int(min=1, max=90) }},"overage_rate":{{ vars.overage_rate }}}"#)
                .delay("50..150"),
            get("Reports", "/usage/daily", "Daily usage", &list(7, r#"{"day":{{ i + 1 }},"api_calls":{{ random_int(min=100, max=9000) }}}"#)),
            get("Reports", "/usage/tenants/:id", "Tenant usage", r#"{"tenant_id":"{{ request.params.id }}","api_calls":{{ random_int(min=100, max=90000) }},"overage_cost":{{ random_int(min=0, max=40) }}.00}"#)
                .when("Tenant unknown", 404, &[("param", "id", "eq", "999")], r#"{"error":"tenant not found"}"#),
            get("Reports", "/usage/limits/:meter", "Meter limit", r#"{"meter":"{{ request.params.meter }}","included":100000,"hard_limit":false}"#),
            post("Events", "/usage/batch", "Batch usage", 202, r#"{"accepted":{{ request.json.count | default(value=0) }}}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"usage"}"#),
        ]);

    let webhooks = Srv::new("webhooks", "Webhooks", 4006)
        .folders(&["Endpoints", "Deliveries"])
        .ds("endpoints", 10, &["id:sequence", "url:template:https://hooks.example.test/{{ uuid() }}", "events:one_of:invoice.paid|subscription.created|usage.alert", "enabled:bool"])
        .routes(vec![
            crud("Endpoints", "/endpoints", "Webhook endpoints", "endpoints"),
            post("Deliveries", "/events", "Event intake", 202, r#"{"queued":true,"event":"{{ request.json.type | default(value='unknown') }}"}"#),
            post("Deliveries", "/deliveries/test", "Send test delivery (with retries)", 202, r#"{"delivery":"dlv_{{ uuid() }}","status":"queued"}"#)
                .when("Endpoint disabled", 409, &[("body", "$.endpoint", "eq", "disabled")], r#"{"error":"endpoint disabled"}"#)
                .cb(Cb { name: "Deliver to tenants", url: "{{ vars.base_url_tenants }}/hooks/inbound".into(), delay_ms: 400, retries: 3, body: r#"{"type":"test.delivery","id":"{{ uuid() }}"}"#.into() }),
            get("Deliveries", "/deliveries", "Recent deliveries", &list(5, r#"{"id":"dlv_{{ uuid() }}","status":"delivered","attempts":{{ random_int(min=1, max=3) }}}"#)),
            get("Deliveries", "/deliveries/:id", "Delivery detail", r#"{"id":"{{ request.params.id }}","status":"delivered","attempts":1,"last_status_code":200}"#)
                .when("Delivery missing", 404, &[("param", "id", "eq", "missing")], r#"{"error":"delivery not found"}"#),
            post("Deliveries", "/deliveries/:id/retry", "Retry delivery", 202, r#"{"id":"{{ request.params.id }}","status":"retrying"}"#),
            get("Endpoints", "/events/types", "Event types", r#"["subscription.created","subscription.cancelled","invoice.paid","invoice.failed","usage.alert"]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"webhooks"}"#),
        ]);

    Ex {
        name: "saas-billing",
        title: "Multi-tenant SaaS billing",
        story: "A multi-tenant SaaS backend: OAuth2 token and refresh with 401 and 403 rules, tenants and members, subscriptions, invoices, metered usage, and a webhooks Server whose Callbacks retry up to three times against a sibling Server.",
        servers: vec![auth, tenants, subscriptions, invoices, usage, webhooks],
        curls: vec![
            ("Get an OAuth2 token", "curl -s -X POST http://127.0.0.1:4001/oauth/token -H 'content-type: application/json' -d '{\"grant_type\":\"client_credentials\",\"client_secret\":\"demo\"}'".into()),
            ("Start a subscription (fires a webhook Callback with 3 retries)", "curl -s -X POST http://127.0.0.1:4003/subscriptions/start -H 'content-type: application/json' -d '{\"plan\":\"team\"}'".into()),
            ("Admin route without a role returns 403", "curl -si http://127.0.0.1:4001/admin/users -H 'authorization: Bearer demo' -H 'x-role: member'".into()),
        ],
        files: vec![],
    }
}
