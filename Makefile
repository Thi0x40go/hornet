PREFIX ?= $(HOME)/.local
BIN_DIR ?= $(PREFIX)/bin

.PHONY: all build run install clean

all: build

build:
	@echo "==> Building Hornet (Pure Rust + Ratatui)..."
	@mkdir -p bin
	cargo build --release
	cp target/release/hornet bin/hornet
	@echo "==> Build complete: bin/hornet"

run: build
	./bin/hornet

install: build
	@echo "==> Installing Hornet to $(BIN_DIR)..."
	@mkdir -p $(BIN_DIR)
	install -m 755 bin/hornet $(BIN_DIR)/hornet
	@echo "Hornet successfully installed to $(BIN_DIR)/hornet!"
	@echo "Make sure $(BIN_DIR) is in your PATH."

clean:
	@echo "==> Cleaning build artifacts..."
	rm -rf bin target

