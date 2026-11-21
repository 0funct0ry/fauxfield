# IoT fleet management

A fleet of connected devices: manage devices, ingest and read telemetry, roll out firmware, raise alerts, track geofences and watch a live WebSocket stream of readings.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Devices (`servers/devices.toml`) | 4001 | 9 |
| Telemetry (`servers/telemetry.toml`) | 4002 | 9 |
| Firmware (`servers/firmware.toml`) | 4003 | 9 |
| Alerts (`servers/alerts.toml`) | 4004 | 9 |
| Geofences (`servers/geofences.toml`) | 4005 | 9 |
| Stream (`servers/stream.toml`) | 4006 | 8 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

List devices:

```bash
curl -s http://127.0.0.1:4001/devices
```

Send a command to a device:

```bash
curl -s -X POST http://127.0.0.1:4001/devices/7/command -H 'content-type: application/json' -d '{"command":"reboot"}'
```

Latest time series (a points value above 500 returns 400):

```bash
curl -s 'http://127.0.0.1:4002/telemetry/7/series?points=24'
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
