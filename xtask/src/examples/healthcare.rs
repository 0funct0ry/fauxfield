use crate::exgen::*;

pub fn ex() -> Ex {
    let patients = Srv::new("patients", "Patients", 4001)
        .folders(&["Patients", "Records"])
        .ds("patients", 40, &["id:sequence", "name:fake:name", "email:fake:internet.email", "phone:fake:phone", "birth_year:int:1940:2015", "blood_type:one_of:A+|A-|B+|B-|O+|O-|AB+"])
        .routes(vec![
            crud("Patients", "/patients", "Patients", "patients"),
            get("Patients", "/patients/:id/summary", "Patient summary", r#"{"id":"{{ request.params.id }}","name":"{{ fake(kind="name") }}","allergies":["pollen"],"conditions":["hypertension"]}"#)
                .when("Patient not found", 404, &[("param", "id", "eq", "999")], r#"{"error":"patient not found"}"#)
                .when("Record restricted", 403, &[("param", "id", "eq", "666")], r#"{"error":"record restricted"}"#),
            get("Records", "/patients/:id/allergies", "Allergies", r#"[{"substance":"pollen","severity":"mild"},{"substance":"penicillin","severity":"severe"}]"#),
            get("Records", "/patients/:id/vitals", "Vitals", r#"{"patient_id":"{{ request.params.id }}","heart_rate":{{ random_int(min=55, max=100) }},"systolic":{{ random_int(min=100, max=140) }},"diastolic":{{ random_int(min=60, max=90) }},"temp_c":36.{{ random_int(min=2, max=9) }}}"#)
                .delay("30..100"),
            get("Records", "/patients/:id/history", "Visit history", &list(3, r#"{"date":"{{ fake(kind="date.past") }}","note":"{{ fake(kind="lorem.sentence") }}"}"#)),
            post("Patients", "/patients/register", "Register patient", 201, r#"{"id":"pat_{{ uuid() }}","name":"{{ request.json.name | default(value='Pat Example') }}","status":"registered"}"#)
                .when("Name missing", 422, &[("body", "$.name", "absent", "")], r#"{"error":"name is required"}"#)
                .when("Duplicate", 409, &[("body", "$.name", "eq", "Duplicate Example")], r#"{"error":"patient already registered"}"#),
            post("Records", "/patients/:id/notes", "Add clinical note", 201, r#"{"id":"note_{{ uuid() }}","patient_id":"{{ request.params.id }}"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"patients"}"#),
        ]);

    let appointments = Srv::new("appointments", "Appointments", 4002)
        .folders(&["Appointments", "Slots"])
        .ds("appointments", 30, &["id:sequence", "patient_id:int:1:40", "provider_id:int:1:12", "status:one_of:scheduled|checked_in|completed|cancelled|no_show", "starts:datetime", "reason:one_of:checkup|follow-up|vaccination|consultation"])
        .routes(vec![
            crud("Appointments", "/appointments", "Appointments", "appointments"),
            get("Slots", "/slots", "Available slots", &list(6, r#"{"slot":"{{ fake(kind="date.future") }}","provider_id":{{ random_int(min=1, max=12) }}}"#))
                .when("Provider missing", 400, &[("query", "provider", "absent", "")], r#"{"error":"provider query parameter is required"}"#)
                .delay("40..140"),
            post("Appointments", "/appointments/book", "Book appointment", 201, r#"{"id":"apt_{{ uuid() }}","status":"scheduled","provider_id":{{ request.json.provider_id | default(value=1) }}}"#)
                .when("Slot taken", 409, &[("body", "$.slot", "eq", "taken")], r#"{"error":"slot no longer available"}"#)
                .when("Patient missing", 422, &[("body", "$.patient_id", "absent", "")], r#"{"error":"patient_id is required"}"#),
            post("Appointments", "/appointments/:id/check-in", "Check in", 200, r#"{"id":"{{ request.params.id }}","status":"checked_in"}"#)
                .when("Cancelled", 409, &[("param", "id", "eq", "13")], r#"{"error":"appointment is cancelled"}"#),
            post("Appointments", "/appointments/:id/cancel", "Cancel", 200, r#"{"id":"{{ request.params.id }}","status":"cancelled"}"#)
                .when("Too late", 409, &[("param", "id", "lte", "3")], r#"{"error":"cannot cancel within 2 hours"}"#),
            post("Appointments", "/appointments/:id/reschedule", "Reschedule", 200, r#"{"id":"{{ request.params.id }}","starts":"{{ fake(kind="date.future") }}"}"#),
            get("Slots", "/appointments/reminders", "Reminders", &list(3, r#"{"appointment":{{ i + 1 }},"channel":"sms","phone":"{{ fake(kind="phone") }}"}"#)),
            get("", "/health", "Health", r#"{"status":"ok","service":"appointments"}"#),
        ]);

    let prescriptions = Srv::new("prescriptions", "Prescriptions", 4003)
        .folders(&["Prescriptions", "Pharmacy"])
        .ds("prescriptions", 30, &["id:sequence", "patient_id:int:1:40", "drug:one_of:Amoxicillin|Lisinopril|Metformin|Atorvastatin|Albuterol|Ibuprofen", "dose_mg:int:5:1000", "refills:int:0:5", "status:one_of:active|filled|expired"])
        .routes(vec![
            crud("Prescriptions", "/prescriptions", "Prescriptions", "prescriptions"),
            post("Prescriptions", "/prescriptions/issue", "Issue prescription", 201, r#"{"id":"rx_{{ uuid() }}","drug":"{{ request.json.drug | default(value='Ibuprofen') }}","refills":2}"#)
                .when("Drug missing", 422, &[("body", "$.drug", "absent", "")], r#"{"error":"drug is required"}"#)
                .when("Interaction", 409, &[("body", "$.drug", "eq", "Warfarin")], r#"{"error":"possible interaction with active prescription"}"#)
                .delay("120..350"),
            post("Prescriptions", "/prescriptions/:id/refill", "Refill", 200, r#"{"id":"{{ request.params.id }}","refills_left":{{ random_int(min=0, max=4) }}}"#)
                .when("No refills left", 409, &[("param", "id", "lte", "3")], r#"{"error":"no refills remaining"}"#),
            get("Prescriptions", "/prescriptions/:id/interactions", "Interaction check", r#"{"id":"{{ request.params.id }}","interactions":[],"checked":"{{ now() }}"}"#),
            get("Pharmacy", "/pharmacies", "Pharmacies", &list(4, r#"{"id":{{ i + 1 }},"name":"{{ fake(kind="company.name") }} Pharmacy","city":"{{ fake(kind="address.city") }}"}"#)),
            post("Pharmacy", "/pharmacy/dispense", "Dispense", 200, r#"{"dispensed":true,"lot":"LOT-{{ random_int(min=10000, max=99999) }}"}"#)
                .when("Expired", 410, &[("body", "$.status", "eq", "expired")], r#"{"error":"prescription expired"}"#),
            get("Pharmacy", "/formulary", "Formulary", r#"["Amoxicillin","Lisinopril","Metformin","Atorvastatin","Albuterol"]"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"prescriptions"}"#),
        ]);

    let labs = Srv::new("labs", "Labs", 4004)
        .folders(&["Orders", "Results"])
        .ds("lab_orders", 25, &["id:sequence", "patient_id:int:1:40", "panel:one_of:CBC|Lipid|Metabolic|Thyroid|HbA1c", "status:one_of:ordered|collected|processing|resulted", "ordered:datetime"])
        .routes(vec![
            crud("Orders", "/lab-orders", "Lab orders", "lab_orders"),
            get("Results", "/results/:order_id", "Result", r#"{"order":"{{ request.params.order_id }}","status":"resulted","values":[{"test":"glucose","value":{{ random_int(min=70, max=140) }},"unit":"mg/dL"}]}"#)
                .when("Not ready", 202, &[("param", "order_id", "gt", "900")], r#"{"status":"processing"}"#)
                .when("Unknown order", 404, &[("param", "order_id", "eq", "0")], r#"{"error":"order not found"}"#)
                .delay("60..250"),
            get("Results", "/results/:order_id/flags", "Abnormal flags", r#"{"order":"{{ request.params.order_id }}","flags":[]}"#),
            post("Orders", "/lab-orders/place", "Place order", 201, r#"{"id":"lab_{{ uuid() }}","panel":"{{ request.json.panel | default(value='CBC') }}","status":"ordered"}"#)
                .when("Panel missing", 422, &[("body", "$.panel", "absent", "")], r#"{"error":"panel is required"}"#),
            post("Orders", "/lab-orders/:id/collect", "Mark collected", 200, r#"{"id":"{{ request.params.id }}","status":"collected"}"#),
            get("Results", "/panels", "Panels", r#"["CBC","Lipid","Metabolic","Thyroid","HbA1c"]"#),
            get("Results", "/reference-ranges", "Reference ranges", r#"{"glucose":{"min":70,"max":99,"unit":"mg/dL"},"hdl":{"min":40,"max":100,"unit":"mg/dL"}}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"labs"}"#),
        ]);

    let insurance = Srv::new("insurance", "Insurance", 4005)
        .var("payer", "Example Mutual")
        .folders(&["Eligibility", "Claims"])
        .ds("claims", 20, &["id:sequence", "patient_id:int:1:40", "amount:float:50:5000", "status:one_of:submitted|approved|denied|paid", "submitted:datetime"])
        .routes(vec![
            crud("Claims", "/claims", "Claims", "claims"),
            post("Eligibility", "/eligibility", "Check eligibility", 200, r#"{"payer":"{{ vars.payer }}","eligible":true,"copay":{{ random_int(min=10, max=50) }},"deductible_remaining":{{ random_int(min=0, max=1500) }}}"#)
                .when("Member id missing", 422, &[("body", "$.member_id", "absent", "")], r#"{"error":"member_id is required"}"#)
                .when("Coverage lapsed", 200, &[("body", "$.member_id", "starts_with", "LAPSED")], r#"{"payer":"{{ vars.payer }}","eligible":false,"reason":"coverage lapsed"}"#)
                .when("Unknown member", 404, &[("body", "$.member_id", "eq", "UNKNOWN")], r#"{"error":"member not found"}"#)
                .delay("100..300"),
            get("Eligibility", "/plans", "Plans", r#"[{"id":"basic","copay":30},{"id":"standard","copay":20},{"id":"premium","copay":10}]"#),
            get("Eligibility", "/members/:id/coverage", "Coverage", r#"{"member":"{{ request.params.id }}","plan":"standard","active":true,"payer":"{{ vars.payer }}"}"#)
                .when("Coverage ended", 410, &[("param", "id", "eq", "ENDED")], r#"{"error":"coverage ended"}"#),
            post("Claims", "/claims/submit", "Submit claim", 201, r#"{"id":"clm_{{ uuid() }}","status":"submitted","amount":{{ request.json.amount | default(value=0) }}}"#)
                .when("Amount invalid", 422, &[("body", "$.amount", "lte", "0")], r#"{"error":"amount must be positive"}"#),
            get("Claims", "/claims/:id/status", "Claim status", r#"{"id":"{{ request.params.id }}","status":"approved","paid":{{ random_int(min=50, max=900) }}.00}"#),
            post("Claims", "/prior-auth", "Prior authorization", 202, r#"{"auth":"pa_{{ uuid() }}","decision":"pending"}"#)
                .when("Not covered", 403, &[("body", "$.procedure", "eq", "cosmetic")], r#"{"error":"procedure not covered"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"insurance"}"#),
        ]);

    let providers = Srv::new("providers", "Providers", 4006)
        .folders(&["Providers", "Facilities"])
        .ds("providers", 12, &["id:sequence", "name:fake:name", "specialty:one_of:cardiology|pediatrics|dermatology|orthopedics|general|neurology", "accepting:bool", "email:fake:internet.email"])
        .ds("facilities", 6, &["id:sequence", "name:fake:company.name", "city:fake:address.city", "phone:fake:phone"])
        .routes(vec![
            crud("Providers", "/providers", "Providers", "providers"),
            crud("Facilities", "/facilities", "Facilities", "facilities"),
            get("Providers", "/providers/search", "Search providers", &list(4, r#"{"id":{{ i + 1 }},"name":"Dr. {{ fake(kind="name.last") }}","specialty":"general","distance_km":{{ random_int(min=1, max=30) }}}"#))
                .when("Specialty missing", 400, &[("query", "specialty", "absent", "")], r#"{"error":"specialty is required"}"#)
                .delay("50..180"),
            get("Providers", "/providers/:id/schedule", "Schedule", r#"{"provider_id":"{{ request.params.id }}","days":["mon","tue","thu"],"hours":"09:00-17:00"}"#)
                .when("Provider on leave", 409, &[("param", "id", "eq", "7")], r#"{"error":"provider on leave"}"#),
            get("Providers", "/providers/:id/credentials", "Credentials", r#"{"provider_id":"{{ request.params.id }}","license":"LIC-{{ random_int(min=100000, max=999999) }}","verified":true}"#),
            get("Providers", "/specialties", "Specialties", r#"["cardiology","pediatrics","dermatology","orthopedics","general","neurology"]"#),
            post("Providers", "/providers/:id/message", "Message provider", 202, r#"{"queued":true,"id":"msg_{{ uuid() }}"}"#)
                .when("Message empty", 422, &[("body", "$.text", "absent", "")], r#"{"error":"text is required"}"#),
            get("", "/health", "Health", r#"{"status":"ok","service":"providers"}"#),
        ]);

    Ex {
        name: "healthcare",
        title: "Health records (synthetic)",
        story: "A synthetic health-records backend: patients, appointments, prescriptions, labs, insurance eligibility and providers. All people, drugs and identifiers are made up; nothing here is real patient data.",
        servers: vec![patients, appointments, prescriptions, labs, insurance, providers],
        curls: vec![
            ("List synthetic patients", "curl -s http://127.0.0.1:4001/patients".into()),
            ("Check insurance eligibility", "curl -s -X POST http://127.0.0.1:4005/eligibility -H 'content-type: application/json' -d '{\"member_id\":\"M-1001\"}'".into()),
            ("A lapsed member is not eligible", "curl -s -X POST http://127.0.0.1:4005/eligibility -H 'content-type: application/json' -d '{\"member_id\":\"LAPSED-7\"}'".into()),
        ],
        files: vec![],
    }
}
