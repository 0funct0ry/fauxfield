---
title: Your first mock
description: Create a Workspace, add a Route with a templated Response and call it.
sidebar:
  order: 2
---

## 1. Create a Workspace

```bash
fauxfield init my-mocks
cd my-mocks
```

`init` writes `fauxfield.toml` and one example Server in `servers/`. Open the app on it with
`fauxfield .`, or stay in the terminal and use the same files.

Want something bigger to explore? `fauxfield init my-shop --example ecommerce` writes a six-Server
Workspace; see [Example Workspaces](/docs/guides/examples/).

## 2. Add a Route

```bash
fauxfield route add users-api GET /users/:id --status 200 --body '{"id": "{{ request.params.id }}", "name": "{{ fake(kind="name") }}"}'
```

In the app: press `Cmd/Ctrl+N`, type the method and path, then edit the Response body. The preview
under the editor renders the template as you type. Either way the change lands in
`servers/users-api.toml`.

## 3. Run it

```bash
fauxfield run
```

```bash
curl localhost:3001/users/42
```

Every call renders new fake data. Set `fake_seed` on the Server (or `fauxfield run --seed 7`) and the
same requests return the same data.

## 4. Check it

```bash
fauxfield validate         # exits 3 when the Workspace has errors
fauxfield logs --follow    # the request log
```

Next: [Workspace layout](/docs/getting-started/workspace-layout/) and
[Routes and matching](/docs/guides/routes/).
