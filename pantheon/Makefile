# Hivemind Plane Picker — Blender add-on dev tasks.
#
# Targets:
#   make install     symlink the add-on into Blender's user extensions dir
#   make uninstall   remove the symlink
#   make test        run unit tests for the pure intent module (no Blender)
#   make lint        ruff check
#   make format      ruff format
#   make build       create a distributable .zip in dist/
#   make clean       remove caches and build artifacts

ADDON_NAME       := hivemind_plane_picker
ADDON_DIR        := $(CURDIR)/$(ADDON_NAME)
BLENDER_VERSION  := 5.1
EXTENSIONS_REPO  := user_default

UNAME := $(shell uname -s)
ifeq ($(UNAME),Darwin)
    BLENDER_EXT_DIR := $(HOME)/Library/Application Support/Blender/$(BLENDER_VERSION)/extensions/$(EXTENSIONS_REPO)
else ifeq ($(UNAME),Linux)
    BLENDER_EXT_DIR := $(HOME)/.config/blender/$(BLENDER_VERSION)/extensions/$(EXTENSIONS_REPO)
else
    BLENDER_EXT_DIR := UNSUPPORTED
endif

INSTALL_PATH := $(BLENDER_EXT_DIR)/$(ADDON_NAME)
VERSION      := $(shell awk -F\" '/^version/ {print $$2}' $(ADDON_NAME)/blender_manifest.toml)

.PHONY: install uninstall test lint format build clean help

help:
	@echo "Available targets: install uninstall test lint format build clean"

install:
	@if [ "$(BLENDER_EXT_DIR)" = "UNSUPPORTED" ]; then \
		echo "Unsupported OS ($(UNAME)). Install manually:"; \
		echo "  Blender → Edit → Preferences → Get Extensions → Install from Disk → $(ADDON_DIR)"; \
		exit 1; \
	fi
	@mkdir -p "$(BLENDER_EXT_DIR)"
	@if [ -e "$(INSTALL_PATH)" ] || [ -L "$(INSTALL_PATH)" ]; then \
		echo "Removing existing $(INSTALL_PATH)"; \
		rm -rf "$(INSTALL_PATH)"; \
	fi
	@ln -s "$(ADDON_DIR)" "$(INSTALL_PATH)"
	@echo "Symlinked $(ADDON_NAME) → $(INSTALL_PATH)"
	@echo "Now enable in Blender: Edit → Preferences → Add-ons → Hivemind Plane Picker"

uninstall:
	@if [ -L "$(INSTALL_PATH)" ] || [ -e "$(INSTALL_PATH)" ]; then \
		rm -rf "$(INSTALL_PATH)"; \
		echo "Removed $(INSTALL_PATH)"; \
	else \
		echo "Not installed at $(INSTALL_PATH)"; \
	fi

test:
	@python3 -m unittest discover -s tests -v

lint:
	@ruff check $(ADDON_NAME) tests

format:
	@ruff format $(ADDON_NAME) tests

build:
	@mkdir -p dist
	@rm -f dist/$(ADDON_NAME)-$(VERSION).zip
	@cd $(ADDON_NAME) && find . -name __pycache__ -type d -exec rm -rf {} + 2>/dev/null || true
	@cd $(ADDON_NAME) && zip -rq ../dist/$(ADDON_NAME)-$(VERSION).zip . -x "*.pyc"
	@echo "Built dist/$(ADDON_NAME)-$(VERSION).zip"

clean:
	@find . -name __pycache__ -type d -exec rm -rf {} + 2>/dev/null || true
	@rm -rf dist build .ruff_cache .pytest_cache .mypy_cache
