# my-up-app

Small demo with two executables that talk SOME/IP through uProtocol.

Build:

```bash
cd /workspaces/docker-uprotocol/my-up-app
cargo build
```

Run the service in one terminal:

```bash
cargo run --bin service
```

Run the client in another terminal:

```bash
cargo run --bin client
```

The crate reuses the local configs and protos from `../up-transport-vsomeip-rust`.
