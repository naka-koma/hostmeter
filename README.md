# hostmeter

A small resident service that samples the load of a machine and serves it as JSON, with a short history,
so that any other program (a dashboard, a web app running in a container) can draw a live graph without
knowing how to read the hardware.

**Status: first working version (0.1.0).** It measures the **GPU** (NVIDIA, on Windows). CPU, memory and disk
are possible later additions under the same contract; nothing else is promised.

## Why

A container has no GPU and cannot read the host's counters, and `nvidia-smi` alone cannot say which program
holds the video memory on Windows (WDDM reports `N/A` per process). `hostmeter` runs on the host, reads
both sources, keeps the last few minutes in memory and answers over HTTP. A client only needs a URL.

## What it measures (first version)

| Source | Gives |
|---|---|
| `nvidia-smi` (the driver's own tool) | GPU utilisation (%), memory used / total, temperature, power draw |
| Windows performance counters `GPU Process Memory` (`Local Usage`) | Video memory per process, with the process name |

It never needs administrator rights, never changes anything on the machine and never writes to disk.

## Contract

The HTTP interface is the product. It is versioned (`/v1/`) and described in [docs/contract.md](docs/contract.md).

```
GET /health
GET /v1/gpu                      the latest sample
GET /v1/gpu/history?seconds=600  the samples of the last N seconds (at most the retention)
```

## Run

```
hostmeter.exe --listen 127.0.0.1:8830 --interval 2s --retention 30m
```

It listens on loopback by default. To reach it from a container on the same machine, use the host gateway
name (for Docker Desktop, `host.docker.internal`); to expose it on a network, bind a specific address
yourself and put your own access control in front of it. There is no authentication in the service.

It is an ordinary console program, so any launcher works: a process manager, a scheduled task, a service
wrapper, or a terminal.

## Build

Written in Rust. `cargo build --release` produces one executable (about 500 KB) with no runtime to install;
`cargo test` runs the tests (they need no GPU).

On the machine it was written for (RTX 5070 Ti), a reading every 2 seconds cost about 0.01 s of CPU per second
and 12 MB of memory.

## Licence

MIT, see [LICENSE](LICENSE).
