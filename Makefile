-include .env
export

.PHONY: lint fix test coverage coverage-text coverage-lcov check zip-projects unpack-projects merge-zip watch \
       docs-version docs-status translate translate-all doc-search doc-search-all \
       prune run-dind-integration

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

# --- Integrated-examples project archive ---
#
# projects.zip is the committed archive; projects/ is gitignored and
# unpacked at test time.  projects.txt is a sorted text manifest of every
# file in the zip so that merge conflicts are human-readable diffs instead
# of binary blobs.
#
# Workflow when two branches both add projects:
#   1. git merge shows a text conflict in projects.txt — resolve it
#      (keep both sides' additions).
#   2. make merge-zip        # extracts ours + theirs zips, unions them
#   3. git merge --continue

# Pack integrated-examples/projects/ into a committable zip and regenerate
# the text manifest.
zip-projects:
	cd integrated-examples && rm -f projects.zip && zip -r projects.zip projects/
	unzip -l integrated-examples/projects.zip \
		| awk 'NR>3 && /^[ ]*[0-9]/ && !/\/$$/ {print $$4}' \
		| sort > integrated-examples/projects.txt

# Unpack projects.zip, replacing any existing projects/ directory.
unpack-projects:
	cd integrated-examples && rm -rf projects && unzip projects.zip

# Merge two conflicting projects.zip archives during a git merge.
# Extracts ours (stage 2) and theirs (stage 3), combines them (union of
# all files, theirs wins on conflict), re-zips, and stages the result.
merge-zip:
	@set -e; \
	WORK=$$(mktemp -d); \
	trap 'rm -rf "$$WORK"' EXIT; \
	echo "==> Extracting ours (stage 2)..."; \
	mkdir -p "$$WORK/ours" "$$WORK/theirs" "$$WORK/merged"; \
	git show :2:integrated-examples/projects.zip > "$$WORK/ours.zip" 2>/dev/null \
		&& unzip -qo "$$WORK/ours.zip" -d "$$WORK/ours" || true; \
	echo "==> Extracting theirs (stage 3)..."; \
	git show :3:integrated-examples/projects.zip > "$$WORK/theirs.zip" 2>/dev/null \
		&& unzip -qo "$$WORK/theirs.zip" -d "$$WORK/theirs" || true; \
	echo "==> Merging (union, theirs wins on conflict)..."; \
	if [ -d "$$WORK/ours/projects" ]; then cp -a "$$WORK/ours/projects/." "$$WORK/merged/projects/"; fi; \
	if [ -d "$$WORK/theirs/projects" ]; then cp -a "$$WORK/theirs/projects/." "$$WORK/merged/projects/"; fi; \
	echo "==> Re-zipping..."; \
	cd "$$WORK/merged" && zip -qr "$(CURDIR)/integrated-examples/projects.zip" projects/; \
	unzip -l "$(CURDIR)/integrated-examples/projects.zip" \
		| awk 'NR>3 && /^[ ]*[0-9]/ && !/\/$$/ {print $$4}' \
		| sort > "$(CURDIR)/integrated-examples/projects.txt"; \
	git add integrated-examples/projects.zip integrated-examples/projects.txt; \
	echo "==> Done. projects.zip and projects.txt merged and staged."

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

# --- Cleanup ---

# Remove build artifacts and Docker leftovers.
#   make prune          cargo clean + DinD docker images
#   make prune DOCKER=1 also run docker system prune
prune:
	cargo clean
	docker rmi coast-dindind-base coast-dindind-integration coast-dindind-wsl-ubuntu 2>/dev/null || true
	docker volume rm coast-dindind-cargo-registry coast-dindind-cargo-git coast-dindind-target coast-dindind-coast-home coast-dindind-docker 2>/dev/null || true
ifdef DOCKER
	docker system prune -a --volumes -f
endif
	@echo "Done. Run 'df -h /Users' to check free space."
	@echo "Note: Docker Desktop may need a restart to compact its disk image."

# --- DinD integration tests ---

# Run integration tests inside a DinD container.
# Usage: make run-dind-integration TEST=test_egress
#        make run-dind-integration TEST=all
run-dind-integration:
	./dindind/integration-runner.sh $(TEST)
