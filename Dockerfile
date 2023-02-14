FROM golang:1.19 AS runj
WORKDIR /usr/src/app/
COPY runj/go.mod runj/go.sum ./
RUN go mod download && go mod verify
COPY runj/ ./
RUN make build

FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
RUN apt update -qq && \
    DEBIAN_FRONTEND=noninteractive apt install -qqy --no-install-recommends pkg-config libdbus-1-dev libsystemd-dev protobuf-compiler
WORKDIR app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin seele

FROM bitnami/minideb:bullseye AS runtime
WORKDIR /etc/seele
RUN install_packages ca-certificates curl gpg gpg-agent && \
    echo 'deb http://download.opensuse.org/repositories/home:/alvistack/Debian_11/ /' | tee /etc/apt/sources.list.d/home:alvistack.list && \
    curl -fsSL https://download.opensuse.org/repositories/home:alvistack/Debian_11/Release.key | apt-key add - > /dev/null && \
    install_packages skopeo umoci uidmap pkg-config libdbus-1-dev libsystemd-dev protobuf-compiler
COPY --from=runj /usr/src/app/bin/runj /usr/local/bin
COPY --from=builder /app/target/release/seele /usr/local/bin
ENTRYPOINT ["/usr/local/bin/seele"]
