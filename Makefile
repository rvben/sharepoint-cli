.PHONY: build release test test-e2e lint fmt check clean install release-patch release-minor release-major

build:
	cargo build

release:
	cargo build --release

test:
	cargo nextest run --lib --bin sharepoint --no-tests=pass
	@if ls tests/*.rs 2>/dev/null | grep -q .; then \
		cargo nextest run --test '*'; \
	fi

# Live e2e tests against a real SharePoint Online tenant.
# Requires SHAREPOINT_E2E_TENANT, SHAREPOINT_E2E_REFRESH_TOKEN,
# SHAREPOINT_E2E_SITE, SHAREPOINT_E2E_LIBRARY.
test-e2e:
	cargo nextest run --test e2e

lint:
	cargo fmt -- --check
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt

check: lint test

clean:
	cargo clean

install: check release
	cp target/release/sharepoint ~/.local/bin/sharepoint

release-patch:
	vership bump patch

release-minor:
	vership bump minor

release-major:
	vership bump major
