# E-commerce platform

Six microservices of an online shop: browse the catalog, fill a cart, place an order, pay (the payments Server fires a webhook Callback to the orders Server), ship and manage users.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Catalog (`servers/catalog.toml`) | 4001 | 8 |
| Cart (`servers/cart.toml`) | 4002 | 7 |
| Orders (`servers/orders.toml`) | 4003 | 8 |
| Payments (`servers/payments.toml`) | 4004 | 7 |
| Shipping (`servers/shipping.toml`) | 4005 | 7 |
| Users (`servers/users.toml`) | 4006 | 7 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

List products:

```bash
curl -s http://127.0.0.1:4001/products
```

Charge a card (watch the webhook Callback in Traffic):

```bash
curl -s -X POST http://127.0.0.1:4004/charges -H 'content-type: application/json' -d '{"amount":4999,"card":"4242424242424242"}'
```

A declined card returns 402:

```bash
curl -si -X POST http://127.0.0.1:4004/charges -H 'content-type: application/json' -d '{"amount":4999,"card":"4000000000000002"}'
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
