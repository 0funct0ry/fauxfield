# Ride-hailing platform

A ride-hailing backend: riders request trips, drivers go online and accept, the trips Server enforces a state machine (requested, accepted, arrived, in progress, completed), pricing applies a surge multiplier from a Server Variable, payments charge the fare and ratings close the loop.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Riders (`servers/riders.toml`) | 4001 | 7 |
| Drivers (`servers/drivers.toml`) | 4002 | 9 |
| Trips (`servers/trips.toml`) | 4003 | 9 |
| Pricing (`servers/pricing.toml`) | 4004 | 8 |
| Payments (`servers/payments.toml`) | 4005 | 8 |
| Ratings (`servers/ratings.toml`) | 4006 | 8 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

Fare estimate with surge (edit the surge Variable on the pricing Server):

```bash
curl -s -X POST http://127.0.0.1:4004/estimate -H 'content-type: application/json' -d '{"pickup":"Central Station"}'
```

Request a trip:

```bash
curl -s -X POST http://127.0.0.1:4003/trips/request -H 'content-type: application/json' -d '{"pickup":"Central Station"}'
```

Completing an already completed trip returns 409:

```bash
curl -si -X POST http://127.0.0.1:4003/trips/3/complete
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
