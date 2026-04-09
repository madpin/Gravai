# Gravai — Audio Capture & AI Meeting Intelligence
# ================================================

.PHONY: help dev build run release clean check check-verbose test lint fmt typecheck install setup reset version

# Default target
help: ## Show this help
	@echo "Gravai Development Commands"
	@echo "==========================="
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

# ── Development ──────────────────────────────────────────────

dev: ## Run in development mode (Rust + Vite hot-reload)
	pnpm tauri:dev

run: ## Build and run the app (debug mode, Vite dev server for frontend)
	pnpm tauri:dev

run-release: ## Build release app bundle and open it
	pnpm tauri build || true
	@APP=$$(find target/release/bundle/macos -name "*.app" -maxdepth 1 2>/dev/null | head -1); \
	if [ -n "$$APP" ]; then \
		echo "✅ Opening $$APP"; \
		open "$$APP"; \
	else \
		echo "❌ No .app found. The DMG step may have failed but the app should still exist."; \
		ls -la target/release/bundle/macos/ 2>/dev/null || true; \
	fi

# ── Build ────────────────────────────────────────────────────

build: ## Build everything (Rust + frontend)
	pnpm build && cargo build --workspace

build-release: ## Build optimized release binary
	pnpm build && cargo build --workspace --release

bundle: ## Build distributable .app + .dmg (needs create-dmg for DMG)
	pnpm tauri build

bundle-app: ## Build .app only (fast, no DMG) with stable cert-based signing
	pnpm tauri build --bundles app
	$(MAKE) sign

sign: ## Sign the built .app with cert-based requirements so TCC permissions persist across updates
	@APP=$$(find target/release/bundle/macos -name "*.app" -maxdepth 1 2>/dev/null | head -1); \
	if [ -n "$$APP" ]; then \
		codesign --force \
			--sign "Gravai Developer Certificate" \
			--requirements '= designated => identifier "com.gravai.app" and certificate leaf = H"FEC8D826B9873249819360FDEB415484D47B0283"' \
			--entitlements src-tauri/Entitlements.plist "$$APP"; \
		echo "✅ Signed with cert-based requirements: $$APP"; \
	else \
		echo "❌ No .app found — run make bundle-app first"; \
	fi

# ── Quality ──────────────────────────────────────────────────

# Set CHECK_VERBOSE=1 for section banners and full Cargo progress (e.g. make check CHECK_VERBOSE=1)
check: ## Pre-push: fmt, clippy, tests, typecheck (silent if ok; CHECK_VERBOSE=1 or make check-verbose for live output)
	@failed=0; \
	if [ -n "$(CHECK_VERBOSE)" ]; then \
		echo "── fmt ──────────────────────────────────────────────"; \
		cargo fmt --all -- --check || failed=1; \
		echo "── clippy ───────────────────────────────────────────"; \
		cargo clippy --workspace -- -D warnings || failed=1; \
		echo "── tests ────────────────────────────────────────────"; \
		cargo test --workspace --lib || failed=1; \
		echo "── typecheck ────────────────────────────────────────"; \
		pnpm typecheck || failed=1; \
	else \
		quiet_run() { \
			out=$$(mktemp); \
			ec=0; \
			"$$@" >"$$out" 2>&1 || ec=$$?; \
			if [ $$ec -ne 0 ]; then cat "$$out"; failed=1; fi; \
			rm -f "$$out"; \
		}; \
		quiet_run cargo fmt --all -- --check; \
		quiet_run cargo clippy --workspace --quiet -- -D warnings; \
		quiet_run cargo test --workspace --lib --quiet; \
		quiet_run pnpm exec svelte-check --tsconfig ./tsconfig.json --output machine; \
	fi; \
	if [ "$$failed" -eq 0 ]; then \
		if [ -n "$(CHECK_VERBOSE)" ]; then echo "✅ All checks passed."; fi; \
	else \
		echo "❌ One or more checks failed."; \
		exit 1; \
	fi

check-verbose: ## Same as check with section banners and non-quiet Cargo output
	@$(MAKE) check CHECK_VERBOSE=1

test: ## Run all tests
	cargo test --workspace --lib

lint: ## Run all linters (clippy + fmt check + svelte-check)
	cargo clippy --workspace -- -D warnings
	cargo fmt --all -- --check
	pnpm typecheck

fmt: ## Auto-format all code
	cargo fmt --all

typecheck: ## Type-check Svelte/TS frontend
	pnpm typecheck

clippy: ## Run Rust clippy lints
	cargo clippy --workspace -- -D warnings

clippy-fix: ## Auto-fix clippy warnings
	cargo clippy --fix --allow-dirty --workspace

# ── Setup ────────────────────────────────────────────────────

install: ## Install all dependencies (Rust + Node)
	rustup update stable
	pnpm install

update-deps: ## Update all dependencies (Rust + Node)
	rustup update stable
	cargo update --workspace
	pnpm update

setup: install ## Full setup (install deps + check build)
	cargo check --workspace
	pnpm build
	@echo "\n✅ Setup complete. Run 'make dev' to start."

# ── Cleanup ──────────────────────────────────────────────────

clean: ## Remove all build artifacts
	cargo clean
	rm -rf dist node_modules/.vite

clean-data: ## Remove Gravai user data (~/.gravai/ and ~/.gravai-dev/)
	rm -rf ~/.gravai ~/.gravai-dev

reset: clean clean-data ## Full reset (build artifacts + user data)
	@echo "✅ Reset complete."

# ── Versioning ───────────────────────────────────────────────

version: ## Bump version: make version V=1.2.3  (omit V to auto-increment patch; runs make check first)
	@if [ -z "$(SKIP_VERSION_CHECK)" ]; then \
		$(MAKE) check || exit 1; \
	fi; \
	if [ -z "$(V)" ]; then \
		CURRENT=$$(grep '^version' Cargo.toml | head -1 | perl -pe 's/version = "(.*)"/$$1/; chomp'); \
		MAJOR=$$(echo $$CURRENT | cut -d. -f1); \
		MINOR=$$(echo $$CURRENT | cut -d. -f2); \
		PATCH=$$(echo $$CURRENT | cut -d. -f3); \
		NEW_V="$$MAJOR.$$MINOR.$$((PATCH + 1))"; \
		echo "Auto-incrementing patch: $$CURRENT → $$NEW_V"; \
		$(MAKE) version V=$$NEW_V SKIP_VERSION_CHECK=1; \
	else \
		echo "Bumping version to $(V)..."; \
		perl -i -pe 's/^version = ".*"/version = "$(V)"/' Cargo.toml; \
		perl -i -pe 's/"version": ".*"/"version": "$(V)"/' src-tauri/tauri.conf.json; \
		perl -i -pe 's/v\d+\.\d+\.\d+/v$(V)/g' src-frontend/components/StatusBar.svelte; \
		cargo update --workspace --quiet; \
		echo "✅ Version updated to $(V) in:"; \
		echo "   Cargo.toml (workspace)"; \
		echo "   src-tauri/tauri.conf.json"; \
		echo "   src-frontend/components/StatusBar.svelte"; \
		echo "   Cargo.lock"; \
	fi

# ── Utility ──────────────────────────────────────────────────

icons: ## Regenerate app icons from the waveform design
	python3 scripts/gen_icon.py /tmp/gravai-icon.png
	pnpm tauri icon /tmp/gravai-icon.png
	cp src-tauri/icons/32x32.png public/icon.png

loc: ## Count lines of code
	@echo "Rust:"
	@find crates src-tauri/src -name '*.rs' -exec cat {} + | wc -l
	@echo "Frontend (Svelte/TS/CSS):"
	@find src-frontend -name '*.svelte' -o -name '*.ts' -o -name '*.css' | xargs cat | wc -l

stats: ## Show project statistics
	@echo "=== Gravai Stats ==="
	@echo "Rust files:    $$(find crates src-tauri/src -name '*.rs' | wc -l | tr -d ' ')"
	@echo "Svelte files:  $$(find src-frontend -name '*.svelte' | wc -l | tr -d ' ')"
	@echo "Tauri commands: $$(grep 'commands::' src-tauri/src/lib.rs | grep -v '//' | wc -l | tr -d ' ')"
	@echo "Tests:          $$(cargo test --workspace --lib 2>&1 | grep 'test result:' | awk -F'[; ]' '{sum += $$4} END {print sum}')"
	@echo "Bundle size:    $$(pnpm build 2>&1 | grep '.js ' | awk '{print $$1, $$3}')"
