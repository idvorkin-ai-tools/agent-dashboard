# Agent Dashboard justfile

# Default recipe - show available commands
default:
    @just --list

# Run development server with hot reload
dev:
    npm run dev-called-from-just

# Build TypeScript to dist/
build:
    npx tsc

# Run production server
start:
    npm run start

# Scan for agent sessions
scan:
    npm run scan

# Install dependencies
install:
    npm install

# Clean build artifacts
clean:
    rm -rf dist

# Rebuild from scratch
rebuild: clean build

# Type check without emitting files
check:
    npx tsc --noEmit

# Watch mode for type checking
watch:
    npx tsc --noEmit --watch

# === TUI Commands ===

# Build TUI
tui-build:
    cd tui && cargo build --release

# Install TUI binary to ~/.cargo/bin
tui-install:
    cargo install --path tui

# Run TUI in dev mode
tui-dev:
    cd tui && cargo run

# Run TUI dump mode (debug)
tui-dump:
    cd tui && cargo run -- --dump

# === Web Deploy ===

# Deploy web UI to Cloudflare Pages
deploy:
    npx wrangler pages deploy public --project-name=agent-dashboard
