# AnchorMAS

AnchorMAS is the AIRS market intelligence project organized as separate workspaces for agent runtime, product UI, design prototypes, and frontend integration backend.

## Workspace Layout

```text
.
|-- agent/            Rust MAS agent runtime and HTTP API
|-- frontend/         React monitoring dashboard for raw data, pipeline, briefing, and agent evolution
|-- frontend-design/  Static product UI prototypes maintained by the design frontend team
`-- backend/          Frontend-facing backend API contract and integration notes
```

## Run Agent API

```bash
cd agent
cp .env.example .env
python3 -m pip install -r scripts/requirements.txt
cargo run
```

The API listens on `SERVER_PORT`, defaulting to `3000`.

## Run Monitoring Frontend

```bash
cd frontend
npm install
npm run dev
```

The Vite dev server listens on `5173` and proxies `/api` requests to `http://localhost:3000`.

## Notes

- Runtime databases, logs, build outputs, and local environment files are ignored.
- `frontend-design/` is kept unchanged as the design team's current static prototype workspace.
- `backend/` stores the frontend-facing API contract and integration notes. The current `/app/*` implementation lives in `agent/src/web/app_handlers.rs`.
