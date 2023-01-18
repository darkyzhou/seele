PKG := runj
BIN_DIR := ./bin

BIN_NAME := runj
BIN_PATH := "$(BIN_DIR)/$(BIN_NAME)"
MAIN_ENTRYPOINT := main.go

GO_VERSION := $(shell go version | cut -d ' ' -f 3)
GO_LDFLAGS += -X "$(PKG)/seele.Version=$(shell git describe --tags --dirty --always)"
GO_LDFLAGS += -X "$(PKG)/seele.GoVersion=$(GO_VERSION)"
GO_LDFLAGS += -X "$(PKG)/seele.BuildTime=$(shell date -u '+%Y-%m-%d_%H:%M:%S%Z')"
GO_LDFLAGS += -X "$(PKG)/seele.GitCommitHash=$(shell git rev-parse HEAD)"
GO_LDFLAGS += -X "$(PKG)/seele.GitBranch=$(shell git rev-parse --abbrev-ref HEAD)"

lint:
	@echo "Linting..."
	@golangci-lint run

test-all:
	@echo "Testing..."
	@go test -v ./...

build: clean
	@echo "Building runj..."
	@CGO_ENABLED=1 GOOS=linux GOARCH=amd64 GO111MODULE=on go build -ldflags '$(GO_LDFLAGS)' -o $(BIN_PATH) $(MAIN_ENTRYPOINT)
	@echo "Successfully built into $(BIN_PATH)"

build-debug: clean
	@echo "Building runj (debug)..."
	@CGO_ENABLED=1 GOOS=linux GOARCH=amd64 GO111MODULE=on go build -tags dev -ldflags '$(GO_LDFLAGS)' -o $(BIN_PATH) $(MAIN_ENTRYPOINT)
	@go mod tidy
	@echo "Successfully built into $(BIN_PATH)"

clean:
	@echo "Cleaning..."
	@go clean
	@rm -f $(BIN_PATH)
