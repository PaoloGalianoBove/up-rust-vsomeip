# Running Client & Service in Separate Docker Containers

## Overview

- **Communication Model**: RPC request/response (sync, with timeout)
- **Discovery**: Two modes available:
  - **TCP hardcoded** (`service_tcp.json`, `client_tcp.json`): Fixed endpoints
  - **Service Discovery** (`service_sd.json`, `client_sd.json`): Dynamic discovery via multicast SD

## Prerequisites

- Docker and Docker Compose installed
- vsomeip libraries compiled within build container

## Quick Start (with Service Discovery)

```bash
cd /workspaces/docker-uprotocol
docker-compose -f my-up-app/docker-compose.yml up
```

This uses Service Discovery by default. Service registers itself; client discovers it via multicast SD.

## Variants

Edit `docker-compose.yml` environment to switch:

```yaml
# Service Discovery (recommended)
VSOMEIP_CONFIG: /app/vsomeip_configs/service_sd.json
VSOMEIP_CONFIG: /app/vsomeip_configs/client_sd.json

# Hardcoded TCP endpoints
VSOMEIP_CONFIG: /app/vsomeip_configs/service_tcp.json
VSOMEIP_CONFIG: /app/vsomeip_configs/client_tcp.json
```

## Configuration Details

### TCP Mode (Hardcoded)
- **service_tcp.json**: Service listens on `0.0.0.0:30491`
- **client_tcp.json**: Client hardcodes endpoint `service:30491`
- No discovery; fixed mapping

### Service Discovery Mode
- **service_sd.json**: Service registers `0x6000:0x0001` and listens on SD multicast `224.244.224.245:30490`
- **client_sd.json**: Client queries SD multicast, discovers service dynamically
- SD port: UDP 30490
- RPC port: TCP 30491

## View Logs

```bash
# All logs
docker-compose logs -f

# Just service
docker-compose logs -f service

# Just client
docker-compose logs -f client
```

## Stop

```bash
docker-compose down
```

## Build & Performance

The Dockerfile is **multi-stage**:
1. **Builder stage**: Compiles Rust+vsomeip once (slow, ~5-10min)
2. **Runtime stage**: Copies only binaries & configs (fast, slim)

Both containers share the same image (compiled once).

## Notes

- RPC calls have 1000ms timeout (see REQUEST_TTL in lib.rs)
- Service Discovery multicast is scoped to the Docker network
- Both modes work in separate containers across different hosts

