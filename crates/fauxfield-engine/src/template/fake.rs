//! `fake(kind=...)`: stable dotted names mapped to the `fake` crate (SPEC §8.2).

use fake::Fake;
use fake::faker::{address::raw as addr, company::raw as comp};
use fake::faker::{chrono::raw as chr, internet::raw as net, lorem::raw as lorem};
use fake::faker::{name::raw as name, phone_number::raw as phone};
use fake::locales::EN;
use rand::{RngExt, rngs::ChaCha8Rng};

/// Every supported kind with a one-line description (also feeds autocomplete and the docs page).
pub const FAKE_KINDS: &[(&str, &str)] = &[
    ("name", "Full name"),
    ("name.first", "First name"),
    ("name.last", "Last name"),
    ("internet.email", "Safe e-mail address"),
    ("internet.username", "User name"),
    ("internet.ipv4", "IPv4 address"),
    ("phone", "Phone number"),
    ("address.street", "Street name"),
    ("address.city", "City name"),
    ("address.country", "Country name"),
    ("address.postcode", "Post code"),
    ("company.name", "Company name"),
    ("lorem.word", "One lorem word"),
    ("lorem.sentence", "One lorem sentence"),
    ("lorem.paragraph", "One lorem paragraph"),
    ("date.past", "RFC 3339 timestamp up to a year in the past"),
    (
        "date.future",
        "RFC 3339 timestamp up to a year in the future",
    ),
    ("number.digit", "A single digit"),
    ("finance.iban", "Syntactically plausible IBAN (GB)"),
    ("finance.bic", "BIC / SWIFT code"),
    (
        "finance.card_number",
        "Card number in the 4000 00 test range",
    ),
    ("color.hex", "Hex colour such as #a1b2c3"),
    ("image.url", "Placeholder image as a service-free data URI"),
];

pub fn generate(kind: &str, rng: &mut ChaCha8Rng) -> Option<String> {
    Some(match kind {
        "name" => name::Name(EN).fake_with_rng(rng),
        "name.first" => name::FirstName(EN).fake_with_rng(rng),
        "name.last" => name::LastName(EN).fake_with_rng(rng),
        "internet.email" => net::SafeEmail(EN).fake_with_rng(rng),
        "internet.username" => net::Username(EN).fake_with_rng(rng),
        "internet.ipv4" => net::IPv4(EN).fake_with_rng(rng),
        "phone" => phone::PhoneNumber(EN).fake_with_rng(rng),
        "address.street" => addr::StreetName(EN).fake_with_rng(rng),
        "address.city" => addr::CityName(EN).fake_with_rng(rng),
        "address.country" => addr::CountryName(EN).fake_with_rng(rng),
        "address.postcode" => addr::PostCode(EN).fake_with_rng(rng),
        "company.name" => comp::CompanyName(EN).fake_with_rng(rng),
        "lorem.word" => lorem::Word(EN).fake_with_rng(rng),
        "lorem.sentence" => lorem::Sentence(EN, 4..10).fake_with_rng(rng),
        "lorem.paragraph" => lorem::Paragraph(EN, 3..6).fake_with_rng(rng),
        "date.past" => {
            let d: chrono::DateTime<chrono::Utc> =
                chr::DateTimeBefore(EN, chrono::Utc::now()).fake_with_rng(rng);
            let year = chrono::Duration::days(365);
            let d = if chrono::Utc::now() - d > year {
                chrono::Utc::now() - chrono::Duration::days(rng.random_range(1..365))
            } else {
                d
            };
            d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        }
        "date.future" => {
            let days = rng.random_range(1..365);
            let secs = rng.random_range(0..86_400);
            (chrono::Utc::now() + chrono::Duration::days(days) + chrono::Duration::seconds(secs))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        }
        "number.digit" => rng.random_range(0..10).to_string(),
        "finance.iban" => {
            let mut s = format!("GB{:02}", rng.random_range(10..99));
            for _ in 0..4 {
                s.push((b'A' + rng.random_range(0..26u8)) as char);
            }
            for _ in 0..14 {
                s.push((b'0' + rng.random_range(0..10u8)) as char);
            }
            s
        }
        "finance.bic" => {
            let mut s = String::new();
            for _ in 0..4 {
                s.push((b'A' + rng.random_range(0..26u8)) as char);
            }
            s.push_str("GB");
            s.push_str(&format!(
                "{}{}",
                (b'A' + rng.random_range(0..26u8)) as char,
                rng.random_range(2..10)
            ));
            s
        }
        "finance.card_number" => {
            let mut s = String::from("400000");
            for _ in 0..10 {
                s.push((b'0' + rng.random_range(0..10u8)) as char);
            }
            s
        }
        "color.hex" => format!("#{:06x}", rng.random_range(0..0x100_0000u32)),
        "image.url" => {
            let c: u32 = rng.random_range(0..0xFF_FFFF);
            format!(
                "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='64' height='64'><rect width='64' height='64' fill='%23{c:06x}'/></svg>"
            )
        }
        _ => return None,
    })
}
