# Build stage
FROM ubuntu:24.04 AS builder

RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    git \
    libboost-all-dev \
    cmake \
    clang \
    g++ \
    cargo \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

# Copy the entire monorepo structure
COPY . .

# Build both binaries
RUN cd my-up-app && cargo build --release 2>&1 | tail -20

# Runtime stage
FROM ubuntu:24.04

RUN apt-get update && apt-get install -y \
    libboost-system1.83.0 \
    netcat-traditional \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy only the compiled binaries from builder
COPY --from=builder /workspace/my-up-app/target/release/my-up-app /app/
COPY --from=builder /workspace/my-up-app/target/release/service /app/bin/
COPY --from=builder /workspace/my-up-app/target/release/client /app/bin/

# Copy config files
COPY --from=builder /workspace/my-up-app/vsomeip_configs /app/vsomeip_configs/

# Copy vsomeip shared libraries if needed
COPY --from=builder /workspace/my-up-app/target/release/deps/*.so* /app/lib/ 2>/dev/null || true

ENV LD_LIBRARY_PATH=/app/lib:/usr/local/lib:/usr/lib
EXPOSE 30491 30492

CMD ["/app/bin/service"]
