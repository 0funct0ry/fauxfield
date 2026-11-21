# Multi-tenant SaaS billing

A multi-tenant SaaS backend: OAuth2 token and refresh with 401 and 403 rules, tenants and members, subscriptions, invoices, metered usage, and a webhooks Server whose Callbacks retry up to three times against a sibling Server.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Auth (`servers/auth.toml`) | 4001 | 9 |
| Tenants (`servers/tenants.toml`) | 4002 | 8 |
| Subscriptions (`servers/subscriptions.toml`) | 4003 | 8 |
| Invoices (`servers/invoices.toml`) | 4004 | 8 |
| Usage (`servers/usage.toml`) | 4005 | 8 |
| Webhooks (`servers/webhooks.toml`) | 4006 | 8 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

Get an OAuth2 token:

```bash
curl -s -X POST http://127.0.0.1:4001/oauth/token -H 'content-type: application/json' -d '{"grant_type":"client_credentials","client_secret":"demo"}'
```

Start a subscription (fires a webhook Callback with 3 retries):

```bash
curl -s -X POST http://127.0.0.1:4003/subscriptions/start -H 'content-type: application/json' -d '{"plan":"team"}'
```

Admin route without a role returns 403:

```bash
curl -si http://127.0.0.1:4001/admin/users -H 'authorization: Bearer demo' -H 'x-role: member'
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
