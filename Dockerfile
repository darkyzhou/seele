ARG GIT_SHA
ARG GIT_NAME

FROM golang:1.23-bookworm AS runj
WORKDIR /usr/src/app/
COPY runj/go.mod runj/go.sum ./
RUN go mod download && go mod verify
COPY runj/ ./
RUN make build

FROM rust:1.83-slim-bookworm AS builder
RUN apt update -qq && \
    DEBIAN_FRONTEND=noninteractive apt install -qqy --no-install-recommends pkg-config libdbus-1-dev libsystemd-dev protobuf-compiler libssl-dev patch
ENV COMMIT_TAG=$GIT_NAME
ENV COMMIT_SHA=$GIT_SHA
WORKDIR /usr/src/seele
COPY . .
RUN cargo install --path .

FROM bitnami/minideb:bookworm AS runtime
WORKDIR /etc/seele
RUN install_packages ca-certificates curl gpg gpg-agent umoci uidmap pkg-config libdbus-1-dev libsystemd-dev protobuf-compiler libssl-dev skopeo
ENV TINI_VERSION=v0.19.0
ADD https://github.com/krallin/tini/releases/download/${TINI_VERSION}/tini-static-amd64 /tini
RUN chmod +x /tini
ENTRYPOINT ["/tini", "--"]
COPY --from=runj /usr/src/app/bin/runj /usr/local/bin
COPY --from=builder /usr/local/cargo/bin/seele /usr/local/bin
CMD ["/usr/local/bin/seele"]
