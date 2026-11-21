# Health records (synthetic)

A synthetic health-records backend: patients, appointments, prescriptions, labs, insurance eligibility and providers. All people, drugs and identifiers are made up; nothing here is real patient data.

## Servers

| Server | Port | Routes |
| --- | --- | --- |
| Patients (`servers/patients.toml`) | 4001 | 8 |
| Appointments (`servers/appointments.toml`) | 4002 | 8 |
| Prescriptions (`servers/prescriptions.toml`) | 4003 | 8 |
| Labs (`servers/labs.toml`) | 4004 | 8 |
| Insurance (`servers/insurance.toml`) | 4005 | 8 |
| Providers (`servers/providers.toml`) | 4006 | 8 |

Two examples are not meant to run together: they reuse the 4001+ port block.

## Run it

```bash
fauxfield validate
fauxfield run
```

Output is deterministic: every Server sets `fake_seed`.

## Try it

List synthetic patients:

```bash
curl -s http://127.0.0.1:4001/patients
```

Check insurance eligibility:

```bash
curl -s -X POST http://127.0.0.1:4005/eligibility -H 'content-type: application/json' -d '{"member_id":"M-1001"}'
```

A lapsed member is not eligible:

```bash
curl -s -X POST http://127.0.0.1:4005/eligibility -H 'content-type: application/json' -d '{"member_id":"LAPSED-7"}'
```

Environments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.
All data is synthetic.
