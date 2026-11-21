# Food delivery

Six services of a delivery app: restaurants, menus, orders, couriers (poll their location), a WebSocket tracking stream that pushes courier positions, and promotions with redeemable codes.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Restaurants (`servers/restaurants.toml`) | 4001 | 8 |
| Menus (`servers/menus.toml`) | 4002 | 8 |
| Orders (`servers/orders.toml`) | 4003 | 8 |
| Couriers (`servers/couriers.toml`) | 4004 | 8 |
| Tracking (`servers/tracking.toml`) | 4005 | 8 |
| Promotions (`servers/promotions.toml`) | 4006 | 8 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

List restaurants:

```bash
curl -s http://127.0.0.1:4001/restaurants
```

Place an order:

```bash
curl -s -X POST http://127.0.0.1:4003/orders/place -H 'content-type: application/json' -d '{"items":[1,2],"total":24.5,"restaurant_id":3}'
```

A closed restaurant returns 409:

```bash
curl -si -X POST http://127.0.0.1:4003/orders/place -H 'content-type: application/json' -d '{"items":[1],"restaurant_id":13}'
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
