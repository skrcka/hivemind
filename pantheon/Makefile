# Hivemind pantheon Blender add-ons — dev tasks.
#
# This Makefile manages two sibling add-ons in this directory:
#   - hivemind_plane_picker  (mark mesh regions, export intent.json)
#   - hivemind_plan_preview  (load a HivemindPlan, build animated scene)
#
# Both install into Blender 5.1's user extensions repo via symlink so edits
# are picked up live.
#
# Targets:
#   make install     symlink every add-on into Blender's user extensions dir
#   make uninstall   remove the symlinks
#   make test        run unit tests for the pure modules (no Blender)
#   make lint        ruff check
#   make format      ruff format
#   make build       create distributable .zip(s) in dist/
#   make clean       remove caches and build artifacts

ADDONS           := hivemind_plane_picker hivemind_plan_preview
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

.PHONY: install uninstall test lint format build clean help

help:
	@echo "Available targets: install uninstall test lint format build clean"
	@echo "Add-ons managed: $(ADDONS)"

install:
	@if [ "$(BLENDER_EXT_DIR)" = "UNSUPPORTED" ]; then \
		echo "Unsupported OS ($(UNAME)). Install each add-on manually:"; \
		for addon in $(ADDONS); do \
			echo "  Blender → Edit → Preferences → Get Extensions → Install from Disk → $(CURDIR)/$$addon"; \
		done; \
		exit 1; \
	fi
	@mkdir -p "$(BLENDER_EXT_DIR)"
	@for addon in $(ADDONS); do \
		target="$(BLENDER_EXT_DIR)/$$addon"; \
		if [ -e "$$target" ] || [ -L "$$target" ]; then \
			echo "Removing existing $$target"; \
			rm -rf "$$target"; \
		fi; \
		ln -s "$(CURDIR)/$$addon" "$$target"; \
		echo "Symlinked $$addon → $$target"; \
	done
	@echo "Now enable in Blender: Edit → Preferences → Add-ons → search 'Hivemind'"

uninstall:
	@for addon in $(ADDONS); do \
		target="$(BLENDER_EXT_DIR)/$$addon"; \
		if [ -L "$$target" ] || [ -e "$$target" ]; then \
			rm -rf "$$target"; \
			echo "Removed $$target"; \
		else \
			echo "Not installed at $$target"; \
		fi; \
	done

test:
	@python3 -m unittest discover -s tests -v

lint:
	@ruff check $(ADDONS) tests

format:
	@ruff format $(ADDONS) tests

build:
	@mkdir -p dist
	@for addon in $(ADDONS); do \
		version=$$(awk -F\" '/^version/ {print $$2}' $$addon/blender_manifest.toml); \
		zip_name="dist/$$addon-$$version.zip"; \
		rm -f "$$zip_name"; \
		(cd $$addon && find . -name __pycache__ -type d -exec rm -rf {} + 2>/dev/null || true); \
		(cd $$addon && zip -rq "../$$zip_name" . -x "*.pyc"); \
		echo "Built $$zip_name"; \
	done

clean:
	@find . -name __pycache__ -type d -exec rm -rf {} + 2>/dev/null || true
	@rm -rf dist build .ruff_cache .pytest_cache .mypy_cache
