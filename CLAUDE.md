# AnchorMAS Guidelines

This workspace contains:
- `agent/`: Rust MAS agent runtime and HTTP API
- `frontend/`: React monitoring dashboard for raw data, pipeline, briefing, and agent evolution
- `frontend-design/`: Static product UI prototypes

## Command Reference

### Build and Run Agent
- Build: `cd agent && cargo build`
- Run: `cd agent && cargo run`
- Test: `cd agent && cargo run --bin test_features` or `cargo test`

### Run Frontend
- Install: `cd frontend && npm install`
- Dev server: `cd frontend && npm run dev`
- Build production: `cd frontend && npm run build`

## Code Guidelines

- Formatting: Use `cargo fmt` for Rust files.
- Styles: React UI uses standard CSS layouts matching design prototypes.
- SQLite: Backend storage uses SQLx with runtime migrations.
