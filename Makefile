.DEFAULT_GOAL := build

PROJECT_NAME := $(notdir $(CURDIR))
BIN_NAME := gallendar
CONSTRUCTION_SIDE ?= $(HOME)/construction_side
CARGO_TARGET_DIR ?= $(CONSTRUCTION_SIDE)/$(PROJECT_NAME)/target
DIST_DIR ?= $(CONSTRUCTION_SIDE)/$(PROJECT_NAME)/dist
INSTALL_DIR ?= $(HOME)/.x-cli-gallendar
TARGET ?= $(shell rustc -vV | sed -n 's/^host: //p')
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
ARGS ?=
export CARGO_TARGET_DIR

.PHONY: install install-local deps-update build build-release build-local typecheck lint fmt test check run version package-release smoke-install release release-push

install:
	cargo fetch --locked

deps-update:
	cargo update

build:
	cargo build --locked

build-release:
	cargo build --release --locked

build-local: build-release
	cp "$(CARGO_TARGET_DIR)/release/$(BIN_NAME)" "$(CURDIR)/$(BIN_NAME)"

typecheck:
	cargo check --locked --all-targets

lint:
	cargo clippy --locked --all-targets -- -D warnings

fmt:
	cargo fmt --all

test:
	cargo test --locked --all-targets

check:
	cargo fmt --all -- --check
	cargo check --locked --all-targets
	cargo clippy --locked --all-targets -- -D warnings
	cargo test --locked --all-targets
	cargo build --release --locked
	@for script in scripts/*.sh; do sh -n "$$script"; done
	@sh scripts/install.sh --help >/dev/null
	git diff --check
	git diff --cached --check

run:
	cargo run --locked -- $(ARGS)

version:
	cargo run --locked -- --version

install-local: build-release
	sh scripts/install-local.sh "$(CARGO_TARGET_DIR)/release/$(BIN_NAME)" "$(INSTALL_DIR)"

package-release: build-release
	sh scripts/package-release.sh "$(TARGET)" "$(VERSION)" "$(DIST_DIR)"

smoke-install: package-release
	sh scripts/smoke-install.sh "$(TARGET)" "$(VERSION)" "$(DIST_DIR)"

release:
	sh scripts/release.sh

release-push:
	@set -eu; \
	branch="$$(git branch --show-current)"; \
	test "$$branch" = main || { echo "ERROR: releases must be pushed from main" >&2; exit 1; }; \
	test -z "$$(git status --porcelain)" || { echo "ERROR: worktree must be clean" >&2; exit 1; }; \
	version="$$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"; \
	git rev-parse -q --verify "refs/tags/v$$version" >/dev/null || { echo "ERROR: missing v$$version. Run make release." >&2; exit 1; }; \
	test "$$(git rev-parse "v$$version^{}")" = "$$(git rev-parse HEAD)" || { echo "ERROR: release tag must point to HEAD" >&2; exit 1; }; \
	git push origin main --follow-tags

.PHONY: vibe-kernel-path vibe-kernel-set vibe-pull

vibe-kernel-path:
	@test -f .vibe/KERNEL_SOURCE || { echo "Missing .vibe/KERNEL_SOURCE. Run: make vibe-kernel-set" >&2; exit 1; }
	@sed -n '1p' .vibe/KERNEL_SOURCE

vibe-kernel-set:
	@mkdir -p .vibe; \
	if [ -n "$(KERNEL)" ]; then kernel_root="$(KERNEL)"; else printf "Kernel path: "; read -r kernel_root; fi; \
	case "$$kernel_root" in /*) ;; *) echo "ERROR: kernel path must be absolute." >&2; exit 1;; esac; \
	test -f "$$kernel_root/tools/vibe-pull" || { echo "ERROR: invalid kernel path: $$kernel_root" >&2; exit 1; }; \
	printf "%s\n" "$$kernel_root" > .vibe/KERNEL_SOURCE

vibe-pull:
	@test -f .vibe/KERNEL_SOURCE || { echo "Missing .vibe/KERNEL_SOURCE. Run: make vibe-kernel-set" >&2; exit 1; }
	@kernel_root="$$(sed -n '1p' .vibe/KERNEL_SOURCE)"; \
	python3 "$$kernel_root/tools/vibe-pull" .

# VIBE:KERNEL_MAKE_START

.PHONY: vibe-propose

vibe-propose:
	@test -f .vibe/KERNEL_SOURCE || { echo "Missing .vibe/KERNEL_SOURCE. Run: make vibe-kernel-set" >&2; exit 1; }
	@kernel_root="$$(sed -n '1p' .vibe/KERNEL_SOURCE)"; \
	test -f "$$kernel_root/tools/vibe-propose" || { echo "Missing $$kernel_root/tools/vibe-propose. Update the kernel source first." >&2; exit 1; }; \
	python3 "$$kernel_root/tools/vibe-propose" .

# VIBE:KERNEL_MAKE_END
