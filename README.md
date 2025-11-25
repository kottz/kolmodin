# 👾 Kolmodin

An interactive multiplayer game suite where your Twitch chat plays along in real time.

## How to Play

Run Kolmodin while streaming on Twitch. Open the web UI, choose a game mode, and enter the channel to listen to. The backend connects to Twitch IRC; viewers play by chatting. The host starts rounds and drives the flow.

## Current Games

- **Deal or No Deal**
- **Med Andra Ord**
- **Quiz**
- **Clip Queue**

## Host Your Own

### Backend Setup

```bash
git clone https://github.com/kottz/kolmodin
cd server
cp env.example .env
cargo run --release
```

See `env.example` for Twitch credentials, data source settings, game toggles, and the optional YouTube key for Clip Queue.

### Frontend Setup

```bash
cd frontend
cp env.example .env.development
cp env.example .env.production
npm install
npm run dev -- --host    # Development
npm run build            # Production
```

Point the public API/WS URLs at wherever the Rust server is running (see `env.example`).

### Creating Custom Questions

Set the backend data source in `.env` (file path or HTTP URL). See `server/kolmodin_data_example.json` for an example file. Use `GET /api/refresh-words` with the admin API key to reload updated data without restarting.
