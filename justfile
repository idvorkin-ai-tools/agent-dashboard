# Agent Dashboard justfile

# Default recipe - show available commands
default:
    @just --list

# Run development server with hot reload
dev:
    npm run dev-called-from-just

# Build TypeScript to dist/
build:
    npm run build-called-from-just

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
