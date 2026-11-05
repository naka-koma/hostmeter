# HTTP contract, version 1

All bodies are JSON (`application/json; charset=utf-8`). Unknown fields must be ignored by a client, so a
field can be added without a new version. A field is removed or changes meaning only under `/v2/`.

## `GET /health`

`200` with `{"status":"ok","version":"0.1.0","uptime_seconds":123}` while the service runs. It says nothing
about the GPU.

## `GET /v1/gpu`

The latest sample.

```json
{
  "schema": 1,
  "sampled_at": "2026-11-06T06:12:09Z",
  "interval_ms": 2000,
  "available": true,
  "adapters": [
    {
      "index": 0,
      "name": "NVIDIA GeForce RTX 5070 Ti",
      "utilization_percent": 3,
      "memory": { "used_mib": 5560, "total_mib": 16303 },
      "temperature_c": 40,
      "power_watts": 37.4,
      "processes": [
        { "pid": 23728, "name": "python.exe", "memory_mib": 2836 }
      ]
    }
  ]
}
```

- `sampled_at` is UTC (RFC 3339). `interval_ms` is how often the service samples.
- `available` is `false` when nothing could be read (no NVIDIA driver, no GPU). Then `adapters` is `[]`
  and the answer is still `200`: the service is up, the hardware is not readable.
- `utilization_percent`, `temperature_c`, `power_watts` may be `null` when the driver does not report them.
- `memory.used_mib` is what the driver reports for the whole adapter (every program, the desktop included).
- `processes` is the video memory each process holds in the adapter's own memory (the Windows counter
  `GPU Process Memory` / `Local Usage`), largest first, at most 20 entries, entries under 16 MiB left out.
  **It does not add up to `memory.used_mib`**: on the machine this was written for, the processes came to about
  80% of it (4.6 of 5.6 GB); the rest is memory the driver and the system hold for themselves. Use it to see
  *who*, not to check the sum. (The counter `Dedicated Usage` was tried and rejected: it counts far more than the
  adapter has.) In version 1 the list belongs to the first adapter; a machine with more than one adapter gets the
  list of the adapter whose processes hold the most.
- A process that ended between the two readings may have no name (`""`).

## `GET /v1/gpu/history?seconds=N`

The samples of the last `N` seconds, oldest first. `N` defaults to the whole retention and is capped at it
(`400` for a value that is not a positive integer).

```json
{
  "schema": 1,
  "interval_ms": 2000,
  "retention_seconds": 1800,
  "samples": [
    {
      "t": "2026-11-06T06:12:07Z",
      "adapters": [
        { "index": 0, "utilization_percent": 3, "memory_used_mib": 5560, "temperature_c": 40, "power_watts": 37.4,
          "processes": [ { "pid": 23728, "name": "python.exe", "memory_mib": 2836 } ] }
      ]
    }
  ]
}
```

A history sample leaves out the adapter's name and total memory (read them from `/v1/gpu`). The service
keeps the history in memory only: it starts empty after a restart.

## Errors

A plain `{"error":"..."}` with the status code (`400`, `404`, `500`). The service never answers `503` for a
missing GPU (see `available`).

## Stability

The shape above is the contract. A client may rely on the field names and types, on the order (oldest first)
and on the `null` rules. It must not rely on the exact number of history samples (a sample can be skipped
when a reading is slow) or on the order of `processes` beyond "largest first".
