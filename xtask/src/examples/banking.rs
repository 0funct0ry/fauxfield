use crate::exgen::*;

pub fn ex() -> Ex {
    let accounts = Srv::new("accounts", "Accounts", 4001)
        .var("bank", "Fauxfield Demo Bank")
        .folders(&["Accounts", "Holders", "Statements"])
        .ds("accounts", 20, &["id:sequence", "holder:fake:name", "iban:fake:finance.iban", "type:one_of:checking|savings|business", "balance:float:0:25000", "currency:one_of:USD|EUR|GBP", "status:one_of:active|dormant|closed"])
        .ds("holders", 12, &["id:sequence", "name:fake:name", "email:fake:internet.email", "phone:fake:phone", "city:fake:address.city"])
        .routes(vec![
            crud("Accounts", "/accounts", "Accounts", "accounts"),
            crud("Holders", "/holders", "Account holders", "holders"),
            get("Accounts", "/accounts/:id/balance", "Balance", r#"{"account_id":"{{ request.params.id }}","available":{{ random_int(min=100, max=20000) }}.25,"currency":"USD","as_of":"{{ now() }}"}"#)
                .when("Unknown account", 404, &[("param", "id", "eq", "0")], r#"{"error":"account not found"}"#)
                .delay("20..80"),
            get("Accounts", "/accounts/:id/summary", "Account summary", r#"{"account_id":"{{ request.params.id }}","iban":"{{ fake(kind="finance.iban") }}","owner":"{{ fake(kind="name") }}","opened":"{{ fake(kind="date.past") }}"}"#),
            get("Statements", "/accounts/:id/statements", "Statements", &list(3, r#"{"month":"2025-{{ i + 1 }}","opening":{{ random_int(min=500, max=9000) }}.00,"closing":{{ random_int(min=500, max=9000) }}.00}"#))
                .delay("60..150"),
            get("Statements", "/statements/:id", "Get statement", r#"{"id":"{{ request.params.id }}","pages":{{ random_int(min=1, max=6) }},"download":"http://127.0.0.1:4001/statements/{{ request.params.id }}/pdf"}"#)
                .when("Statement not found", 404, &[("param", "id", "eq", "missing")], r#"{"error":"statement not found"}"#),
            post("Accounts", "/accounts/:id/close", "Close account", 200, r#"{"account_id":"{{ request.params.id }}","status":"closed"}"#)
                .when("Balance not zero", 409, &[("body", "$.balance", "gt", "0")], r#"{"error":"balance must be zero to close"}"#),
            get("Holders", "/holders/:id/accounts", "Holder accounts", &list(2, r#"{"id":{{ i + 1 }},"iban":"{{ fake(kind="finance.iban") }}","type":"checking"}"#)),
            get("", "/health", "Health", r#"{"status":"ok","service":"accounts"}"#),
        ]);

    let transactions = Srv::new("transactions", "Transactions", 4002)
        .folders(&["Transactions", "Reports"])
        .ds("transactions", 60, &["id:sequence", "account_id:int:1:20", "amount:float:1:2000", "direction:one_of:debit|credit", "merchant:fake:company.name", "booked_at:datetime", "category:one_of:groceries|rent|salary|travel|utilities"])
        .routes(vec![
            crud("Transactions", "/transactions", "Transactions", "transactions"),
            get("Transactions", "/accounts/:id/transactions", "Account transactions (paginated)", r#"{"page":{{ request.query.page | default(value=1) }},"page_size":3,"total":57,"items":[{"id":"{{ uuid() }}","amount":{{ random_int(min=1, max=900) }}.40,"merchant":"{{ fake(kind="company.name") }}","booked_at":"{{ fake(kind="date.past") }}"},{"id":"{{ uuid() }}","amount":{{ random_int(min=1, max=900) }}.15,"merchant":"{{ fake(kind="company.name") }}","booked_at":"{{ fake(kind="date.past") }}"},{"id":"{{ uuid() }}","amount":{{ random_int(min=1, max=900) }}.80,"merchant":"{{ fake(kind="company.name") }}","booked_at":"{{ fake(kind="date.past") }}"}]}"#)
                .when("Page out of range", 400, &[("query", "page", "gt", "19")], r#"{"error":"page out of range"}"#)
                .delay("30..90"),
            get("Reports", "/transactions/search", "Filter by date", &list(3, r#"{"id":"{{ uuid() }}","booked_at":"{{ request.query.from | default(value='2025-01-01') }}","amount":{{ random_int(min=1, max=500) }}.00}"#))
                .when("Invalid range", 400, &[("query", "from", "absent", "")], r#"{"error":"query parameter from is required"}"#),
            get("Reports", "/transactions/summary", "Monthly summary", r#"{"credits":{{ random_int(min=2000, max=9000) }}.00,"debits":{{ random_int(min=1000, max=8000) }}.00,"count":{{ random_int(min=20, max=120) }}}"#),
            get("Transactions", "/transactions/:id/receipt", "Receipt", r#"{"id":"{{ request.params.id }}","merchant":"{{ fake(kind="company.name") }}","issued":"{{ fake(kind="date.past") }}"}"#)
                .when("No receipt", 404, &[("param", "id", "eq", "none")], r#"{"error":"receipt not available"}"#),
            post("Transactions", "/transactions/:id/dispute", "Dispute transaction", 201, r#"{"dispute_id":"dsp_{{ uuid() }}","transaction":"{{ request.params.id }}","status":"open"}"#)
                .when("Reason missing", 422, &[("body", "$.reason", "absent", "")], r#"{"error":"reason is required"}"#),
            get("Reports", "/categories", "Spending categories", r#"["groceries","rent","salary","travel","utilities"]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"transactions"}"#),
        ]);

    let transfers = Srv::new("transfers", "Transfers", 4003)
        .folders(&["Transfers", "Beneficiaries"])
        .ds("beneficiaries", 8, &["id:sequence", "name:fake:name", "iban:fake:finance.iban", "bic:fake:finance.bic"])
        .routes(vec![
            post("Transfers", "/transfers", "Create transfer", 201, r#"{"id":"trf_{{ uuid() }}","amount":{{ request.json.amount | default(value=0) }},"status":"pending","created":"{{ now() }}"}"#)
                .when("Insufficient funds", 422, &[("body", "$.amount", "gt", "10000")], r#"{"error":{"code":"insufficient_funds","message":"Available balance is lower than the amount."}}"#)
                .when("Amount missing", 400, &[("body", "$.amount", "absent", "")], r#"{"error":"amount is required"}"#)
                .delay("100..300"),
            get("Transfers", "/transfers", "List transfers", &list(4, r#"{"id":"trf_{{ uuid() }}","amount":{{ random_int(min=10, max=900) }}.00,"status":"settled"}"#)),
            get("Transfers", "/transfers/:id", "Get transfer", r#"{"id":"{{ request.params.id }}","status":"settled","amount":250.00}"#)
                .when("Transfer not found", 404, &[("param", "id", "eq", "unknown")], r#"{"error":"transfer not found"}"#),
            post("Transfers", "/transfers/:id/cancel", "Cancel transfer", 200, r#"{"id":"{{ request.params.id }}","status":"cancelled"}"#)
                .when("Already settled", 409, &[("param", "id", "starts_with", "settled")], r#"{"error":"transfer already settled"}"#),
            crud("Beneficiaries", "/beneficiaries", "Beneficiaries", "beneficiaries"),
            post("Transfers", "/transfers/validate", "Validate transfer", 200, r#"{"valid":true,"fee":{{ random_int(min=0, max=3) }}.50}"#)
                .when("Bad IBAN", 422, &[("body", "$.iban", "absent", "")], r#"{"valid":false,"error":"iban is required"}"#),
            get("Transfers", "/transfers/limits", "Limits", r#"{"daily":10000,"per_transfer":10000,"currency":"USD"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"transfers"}"#),
        ]);

    let cards = Srv::new("cards", "Cards", 4004)
        .folders(&["Cards", "Controls"])
        .ds("cards", 10, &["id:sequence", "holder:fake:name", "number:fake:finance.card_number", "status:one_of:active|frozen|expired", "limit:int:500:10000"])
        .routes(vec![
            crud("Cards", "/cards", "Cards", "cards"),
            post("Controls", "/cards/:id/freeze", "Freeze card", 200, r#"{"card_id":"{{ request.params.id }}","status":"frozen","at":"{{ now() }}"}"#)
                .when("Card expired", 409, &[("param", "id", "eq", "0")], r#"{"error":"expired cards cannot be frozen"}"#),
            post("Controls", "/cards/:id/unfreeze", "Unfreeze card", 200, r#"{"card_id":"{{ request.params.id }}","status":"active","at":"{{ now() }}"}"#)
                .when("Not frozen", 409, &[("query", "force", "eq", "no")], r#"{"error":"card is not frozen"}"#),
            post("Controls", "/cards/:id/limit", "Set limit", 200, r#"{"card_id":"{{ request.params.id }}","limit":{{ request.json.limit | default(value=1000) }}}"#)
                .when("Limit too high", 422, &[("body", "$.limit", "gt", "20000")], r#"{"error":"limit exceeds product maximum"}"#),
            get("Cards", "/cards/:id/transactions", "Card activity", &list(3, r#"{"merchant":"{{ fake(kind="company.name") }}","amount":{{ random_int(min=3, max=300) }}.00}"#))
                .delay("30..100"),
            post("Cards", "/cards/:id/pin", "Reset PIN", 202, r#"{"card_id":"{{ request.params.id }}","pin_reset":"requested"}"#),
            get("Cards", "/cards/:id/details", "Card details", r#"{"id":"{{ request.params.id }}","number":"{{ fake(kind="finance.card_number") }}","expires":"12/29","brand":"demo"}"#)
                .when("Details locked", 403, &[("header", "x-card-pin", "absent", "")], r#"{"error":"pin required"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"cards"}"#),
        ]);

    let kyc = Srv::new("kyc", "KYC", 4005)
        .folders(&["Cases", "Documents"])
        .ds("cases", 15, &["id:sequence", "applicant:fake:name", "status:one_of:pending|in_review|approved|rejected", "risk:one_of:low|medium|high", "submitted_at:datetime"])
        .routes(vec![
            crud("Cases", "/cases", "KYC cases", "cases"),
            post("Cases", "/cases/:id/submit", "Submit for review", 202, r#"{"case_id":"{{ request.params.id }}","status":"in_review"}"#)
                .when("Documents missing", 422, &[("body", "$.documents", "absent", "")], r#"{"error":"at least one document is required"}"#),
            get("Cases", "/cases/:id/status", "Case status", r#"{"case_id":"{{ request.params.id }}","status":"in_review","updated":"{{ now() }}"}"#)
                .when("Approved case", 200, &[("param", "id", "lte", "5")], r#"{"case_id":0,"status":"approved","updated":"2025-01-02T09:00:00Z"}"#)
                .when("Rejected case", 200, &[("param", "id", "gte", "90")], r#"{"case_id":90,"status":"rejected","reason":"document unreadable"}"#),
            post("Cases", "/cases/:id/approve", "Approve case", 200, r#"{"case_id":"{{ request.params.id }}","status":"approved"}"#)
                .when("High risk needs manager", 403, &[("header", "x-role", "ne", "manager")], r#"{"error":"manager approval required"}"#),
            post("Documents", "/cases/:id/documents", "Upload document", 201, r#"{"document_id":"doc_{{ uuid() }}","type":"{{ request.json.type | default(value='passport') }}","state":"received"}"#)
                .when("Type missing", 422, &[("body", "$.type", "absent", "")], r#"{"error":"type is required"}"#)
                .delay("100..250"),
            get("Documents", "/cases/:id/documents", "List documents", &list(2, r#"{"id":"doc_{{ uuid() }}","type":"proof_of_address","state":"verified"}"#)),
            get("Cases", "/workflow", "Status workflow", r#"["pending","in_review","approved","rejected"]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"kyc"}"#),
        ]);

    let fx = Srv::new("fx", "FX", 4006)
        .var("base", "USD")
        .folders(&["Rates", "Conversions"])
        .ds("pairs", 6, &["id:sequence", "pair:one_of:USD/EUR|USD/GBP|EUR/GBP|USD/JPY|GBP/EUR|EUR/USD", "rate:float:0.5:150"])
        .routes(vec![
            crud("Rates", "/pairs", "Currency pairs", "pairs"),
            get("Rates", "/rates", "Latest rates", r#"{"base":"{{ vars.base }}","as_of":"{{ now() }}","rates":{"EUR":0.{{ random_int(min=90, max=95) }},"GBP":0.{{ random_int(min=77, max=82) }},"JPY":{{ random_int(min=140, max=155) }}.2}}"#)
                .delay("20..60"),
            get("Rates", "/rates/:pair", "Rate for pair", r#"{"pair":"{{ request.params.pair }}","rate":{{ random_int(min=1, max=150) }}.{{ random_int(min=10, max=99) }}}"#)
                .when("Unsupported pair", 404, &[("param", "pair", "eq", "XXX-YYY")], r#"{"error":"unsupported pair"}"#),
            get("Rates", "/rates/:pair/history", "Rate history", &list(5, r#"{"day":"{{ fake(kind="date.past") }}","rate":{{ random_int(min=1, max=150) }}.50}"#)),
            post("Conversions", "/convert", "Convert amount", 200, r#"{"from":"{{ request.json.from | default(value='USD') }}","to":"{{ request.json.to | default(value='EUR') }}","amount":{{ request.json.amount | default(value=0) }},"converted":{{ random_int(min=1, max=900) }}.12}"#)
                .when("Amount missing", 422, &[("body", "$.amount", "absent", "")], r#"{"error":"amount is required"}"#)
                .delay("40..120"),
            post("Conversions", "/quotes", "Lock quote", 201, r#"{"quote_id":"qte_{{ uuid() }}","valid_seconds":30,"rate":0.92}"#),
            get("Rates", "/currencies", "Currencies", r#"["USD","EUR","GBP","JPY","CHF"]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"fx"}"#),
        ]);

    Ex {
        name: "banking",
        title: "Fintech and banking",
        story: "Six services of a demo bank: accounts and holders, a paginated transactions ledger, transfers that reject insufficient funds with a 422, card freeze controls, a KYC status workflow and FX rates.",
        servers: vec![accounts, transactions, transfers, cards, kyc, fx],
        curls: vec![
            ("List accounts", "curl -s http://127.0.0.1:4001/accounts".into()),
            ("Create a transfer", "curl -s -X POST http://127.0.0.1:4003/transfers -H 'content-type: application/json' -d '{\"amount\":250}'".into()),
            ("An oversized transfer returns 422 insufficient funds", "curl -si -X POST http://127.0.0.1:4003/transfers -H 'content-type: application/json' -d '{\"amount\":50000}'".into()),
        ],
        files: vec![],
    }
}
