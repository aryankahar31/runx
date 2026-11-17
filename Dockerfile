# Build:  docker build -t runx .
# Use:    docker run --rm -v "$PWD:/work" -v runx-cache:/runx -w /work runx dev
#
# The image is not published to a registry yet; building locally is the
# supported path. runx still downloads runtimes at run time, so the container
# needs outbound network on first use. Mount /runx to keep the cache.
FROM rust:1.88-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/runx /usr/local/bin/runx
ENV RUNX_HOME=/runx
VOLUME ["/runx"]
ENTRYPOINT ["runx"]
