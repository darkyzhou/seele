ARG GIT_SHA
ARG GIT_NAME

FROM golang:1.19 AS runj
WORKDIR /usr/src/app/
COPY runj/go.mod runj/go.sum ./
RUN go mod download && go mod verify
COPY runj/ ./
RUN make build

FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
RUN apt update -qq && \
    DEBIAN_FRONTEND=noninteractive apt install -qqy --no-install-recommends pkg-config libdbus-1-dev libsystemd-dev protobuf-compiler libssl-dev
WORKDIR app

FROM chef AS planner
ENV COMMIT_TAG=$GIT_NAME
ENV COMMIT_SHA=$GIT_SHA
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ENV COMMIT_TAG=$GIT_NAME
ENV COMMIT_SHA=$GIT_SHA
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin seele

FROM bitnami/minideb:bullseye AS runtime
WORKDIR /etc/seele
RUN install_packages ca-certificates curl gpg gpg-agent umoci uidmap pkg-config libdbus-1-dev libsystemd-dev protobuf-compiler libssl-dev && \
    echo 'deb http://download.opensuse.org/repositories/home:/alvistack/Debian_11/ /' | tee /etc/apt/sources.list.d/home:alvistack.list && \
    curl -fsSL https://download.opensuse.org/repositories/home:alvistack/Debian_11/Release.key | apt-key add - > /dev/null && \
    install_packages skopeo
ENV TINI_VERSION=v0.19.0
ADD https://github.com/krallin/tini/releases/download/${TINI_VERSION}/tini-static-amd64 /tini
RUN chmod +x /tini
ENTRYPOINT ["/tini", "--"]
COPY --from=runj /usr/src/app/bin/runj /usr/local/bin
COPY --from=builder /app/target/release/seele /usr/local/bin
CMD ["/usr/local/bin/seele"]
