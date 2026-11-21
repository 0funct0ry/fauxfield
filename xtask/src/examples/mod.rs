pub mod banking;
pub mod ecommerce;
pub mod food_delivery;
pub mod healthcare;
pub mod iot_fleet;
pub mod ride_hailing;
pub mod saas_billing;
pub mod social;
pub mod streaming_media;
pub mod travel_booking;

pub fn all() -> Vec<crate::exgen::Ex> {
    vec![
        ecommerce::ex(),
        banking::ex(),
        social::ex(),
        food_delivery::ex(),
        ride_hailing::ex(),
        saas_billing::ex(),
        healthcare::ex(),
        iot_fleet::ex(),
        streaming_media::ex(),
        travel_booking::ex(),
    ]
}
