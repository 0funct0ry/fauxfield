# Fintech and banking

Six services of a demo bank: accounts and holders, a paginated transactions ledger, transfers that reject insufficient funds with a 422, card freeze controls, a KYC status workflow and FX rates.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Accounts (`servers/accounts.toml`) | 4001 | 9 |
| Transactions (`servers/transactions.toml`) | 4002 | 8 |
| Transfers (`servers/transfers.toml`) | 4003 | 8 |
| Cards (`servers/cards.toml`) | 4004 | 8 |
| KYC (`servers/kyc.toml`) | 4005 | 8 |
| FX (`servers/fx.toml`) | 4006 | 8 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

List accounts:

```bash
curl -s http://127.0.0.1:4001/accounts
```

Create a transfer:

```bash
curl -s -X POST http://127.0.0.1:4003/transfers -H 'content-type: application/json' -d '{"amount":250}'
```

An oversized transfer returns 422 insufficient funds:

```bash
curl -si -X POST http://127.0.0.1:4003/transfers -H 'content-type: application/json' -d '{"amount":50000}'
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
