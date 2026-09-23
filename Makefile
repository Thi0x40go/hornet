PREFIX ?= $(HOME)/.local
BIN_DIR ?= $(PREFIX)/bin

.PHONY: all build build-engine build-tui run install clean

all: build

build: build-engine build-tui

build-engine:
	@echo "==> Building Hornet Engine (Go)..."
	@mkdir -p bin
	cd engine && go build -o ../bin/hornet-server ./cmd/server

build-tui:
	@echo "==> Building Hornet TUI (Rust + Ratatui)..."
	@mkdir -p bin
	cargo build --release
	cp target/release/hornet bin/hornet

run: build
	./bin/hornet

install: build
	@echo "==> Installing Hornet to $(BIN_DIR)..."
	@mkdir -p $(BIN_DIR)
	install -m 755 bin/hornet $(BIN_DIR)/hornet
	install -m 755 bin/hornet-server $(BIN_DIR)/hornet-server
	@echo "Hornet successfully installed to $(BIN_DIR)!"
	@echo "Make sure $(BIN_DIR) is in your PATH."

clean:
	@echo "==> Cleaning build artifacts..."
	rm -rf bin target
