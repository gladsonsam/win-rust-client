# 🛡 Sentinel

A lightweight, self-hosted monitoring system built with Rust and React. A Windows agent streams real-time telemetry to a high-performance Rust server, which feeds a live web dashboard with screen streaming, keylogging, window/URL tracking, and AFK detection.

## Features

- **Real-time Dashboard** — Live WebSocket feed of window focus, keystrokes, URLs, and activity state.
- **MJPEG Screen Streaming** — Demand-driven screen capture; the agent stops capturing when no viewers are watching.
- **Remote Control** — Send mouse and keyboard commands from the dashboard to the agent.
- **Secure by Default** — Agent auth via shared secret, dashboard via password-protected sessions.
- **PostgreSQL Persistence** — Full historical record of keys, windows, URLs, and activity.
- **Single-container Deploy** — The Rust server embeds the compiled React frontend; no separate web server needed.

## Tech Stack

| Component | Technology |
|---|---|
| **sentinel-agent** | Rust (Windows, egui tray, xcap, enigo) |
| **sentinel-server** | Rust (Axum, Tokio, SQLx, PostgreSQL) |
| **sentinel-dashboard** | React 18, Vite, TailwindCSS |

---

## 🚀 Quick Deploy (Docker + Traefik)

### 1. Configure

```bash
cp env.example .env
```

Edit `.env` and set the required values:

| Variable | Description |
|---|---|
| `POSTGRES_PASSWORD` | Database password (required) |
| `UI_PASSWORD` | Dashboard login password |
| `AGENT_SECRET` | Shared secret for agent authentication |
| `TRAEFIK_HOST` | Your domain (e.g. `sentinel.example.com`) |

### 2. Deploy

```bash
docker compose up -d --build
```

The dashboard will be available at `http://localhost:9000` or your Traefik domain.

---

## 🛠 Development

### Server
```bash
cd server
cargo run
```

### Frontend
```bash
cd frontend
npm install
npm run dev
```

### Agent (Windows, cross-compile from Linux)
```bash
cd agent
cargo xwin build --release
```

---

## Connecting an Agent

Once the server is running, configure the agent on each Windows machine with:

```
AGENT_SERVER_URL=ws://<server>:9000/ws/agent
AGENT_NAME=<hostname>
AGENT_SECRET=<your-secret>
```

The agent will appear in the dashboard automatically on connect.

## Healthcheck

```bash
curl http://127.0.0.1:9000/healthz
# → 200 OK
```

## License

MIT — see [LICENSE](LICENSE).
