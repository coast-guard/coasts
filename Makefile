-include .env
export

.PHONY: lint fix test coverage coverage-text check fmt zip-projects unpack-projects watch \
       docs-version docs-status translate translate-all doc-search doc-search-all

# Check formatting and run clippy. Matches CI: -D warnings promotes all
# warnings to errors so local lint catches what CI catches.
lint:
	cargo fmt --all -- --check
	cargo clippy --workspace -- -D warnings

# Auto-format and auto-fix what clippy can.
fix:
	cargo fmt --all
	cargo clippy --workspace --fix --allow-dirty --allow-staged

# Run the full test suite (excludes #[ignore] integration tests).
test:
	cargo test --workspace

# Generate an HTML coverage report and open it in the browser.
coverage:
	cargo llvm-cov --workspace --html --open

# Print a per-file coverage summary to the terminal.
coverage-text:
	cargo llvm-cov --workspace --text

# Export lcov for CI / codecov integration.
coverage-lcov:
	cargo llvm-cov --workspace --lcov --output-path lcov.info

# Full pre-PR check: format, lint, then test.
check: lint test

# Pack integrated-examples/projects/ (including .git) into a committable zip.
zip-projects:
	cd integrated-examples && rm -f projects.zip && zip -r projects.zip projects/

# Unpack projects.zip, replacing any existing projects/ directory.
unpack-projects:
	cd integrated-examples && rm -rf projects && unzip projects.zip

# Watch all Rust sources and rebuild on changes.
# Install first: cargo install cargo-watch
watch:
	cargo watch -w coast-core -w coast-i18n -w coast-secrets -w coast-docker -w coast-daemon -w coast-cli -x 'build --workspace'

# --- Docs localization ---
LOCALES ?= zh ja ko ru pt es

# Update docs/version.txt with Merkle tree of English docs.
docs-version:
	python3 scripts/docs-i18n.py version

# Show which docs are missing or stale per locale.
docs-status:
	python3 scripts/docs-i18n.py status

# Translate docs for a single locale.  Usage: make translate LOCALE=es
translate:
	python3 scripts/docs-i18n.py translate --locale $(LOCALE)

# Translate docs for every supported locale.
translate-all:
	@for locale in $(LOCALES); do \
		echo "=== Translating $$locale ==="; \
		python3 scripts/docs-i18n.py translate --locale $$locale; \
	done

# --- Docs search index ---

# Generate search index for a single locale.  Usage: make doc-search LOCALE=en
doc-search:
	python3 scripts/generate-search-index.py --locale $(LOCALE)

# Generate search indexes for every locale (en + translations).
doc-search-all:
	@for locale in en $(LOCALES); do \
		echo "=== Indexing $$locale ==="; \
		python3 scripts/generate-search-index.py --locale $$locale; \
	done
