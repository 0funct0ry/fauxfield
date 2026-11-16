---
title: HTTPS
description: Serve a Server over HTTPS with a local development CA or your own certificate files.
---

```toml
[tls]
enabled = true
mode = "dev-ca"            # dev-ca | files
# cert = "certs/server.pem"      # files mode, workspace-relative
# key  = "certs/server-key.pem"
also_http_port = 4311      # optional: the same routes in plain HTTP
```

**dev-ca.** The first HTTPS start creates a local CA in the platform data directory (`~/.local/share/fauxfield/ca/` on Linux; `FAUXFIELD_DATA_DIR` overrides it) and issues a leaf certificate for `localhost`, `127.0.0.1`, `::1` and the Server host. Browsers trust it only after you trust the CA:

```bash
fauxfield certs status        # path, fingerprint, expiry, trust state
fauxfield certs trust         # prints the platform command; --yes runs it
fauxfield certs untrust
```

Fauxfield never edits a trust store silently: the command is printed first, and runs only with `--yes` (or an interactive `y`). In the app, the TLS section's **Trust dev CA…** button shows the exact command and runs it only when you press **Run**.

**files.** PEM certificate and key from the workspace. Expiry is checked when the Server starts and warned 14 days ahead (TLS section, `fauxfield doctor`).

`fauxfield doctor` reports ports, the CA trust state and certificate expiry. HTTPS Servers show a lock icon in the tree.
