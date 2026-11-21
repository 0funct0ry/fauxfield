# Travel booking

A travel agency backend: search flights, hotels and cars, hold then confirm or cancel a booking, pay (the payments Server fires a webhook Callback to the bookings Server) and earn loyalty points.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Flights (`servers/flights.toml`) | 4001 | 9 |
| Hotels (`servers/hotels.toml`) | 4002 | 9 |
| Cars (`servers/cars.toml`) | 4003 | 8 |
| Bookings (`servers/bookings.toml`) | 4004 | 9 |
| Payments (`servers/payments.toml`) | 4005 | 8 |
| Loyalty (`servers/loyalty.toml`) | 4006 | 8 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

Search flights:

```bash
curl -s 'http://127.0.0.1:4001/flights/search?from=BER&to=AMS'
```

Hold a booking:

```bash
curl -s -X POST http://127.0.0.1:4004/bookings/hold -H 'content-type: application/json' -d '{"item":"flight"}'
```

Pay (watch the webhook Callback in Traffic); the card 4000000000000002 returns 402:

```bash
curl -s -X POST http://127.0.0.1:4005/payments -H 'content-type: application/json' -d '{"amount":24900,"card":"4242424242424242"}'
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
