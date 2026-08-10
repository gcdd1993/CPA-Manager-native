PLUGIN_NAME := provider-model-sync
GO ?= go
OUT_DIR ?= bin

.PHONY: test build-linux build-darwin build-windows clean

test:
	$(GO) test ./...

build-linux:
	mkdir -p $(OUT_DIR)
	$(GO) build -buildmode=c-shared -o $(OUT_DIR)/$(PLUGIN_NAME).so .

build-darwin:
	mkdir -p $(OUT_DIR)
	$(GO) build -buildmode=c-shared -o $(OUT_DIR)/$(PLUGIN_NAME).dylib .

build-windows:
	mkdir -p $(OUT_DIR)
	$(GO) build -buildmode=c-shared -o $(OUT_DIR)/$(PLUGIN_NAME).dll .

clean:
	$(GO) clean
