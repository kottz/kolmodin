# 👾 Kolmodin
Kolmodin is an application used to play games through Twitch chat. It supports multiple different game modes. The streamer shows the Kolmodin UI on stream and the viewers participate by typing in the chat.

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

See `env.example` for configuration options.

### Frontend Setup

```bash
cd frontend
cp env.example .env.development
cp env.example .env.production
npm install
npm run dev -- --host    # Development
npm run build            # Production
```

See `env.example` for configuration options.

### Spoof Twitch Chat Locally

Run the bundled IRC shim TUI if you want to test without real Twitch chat:

```bash
# Terminal 1: start the spoof server
cd twitch_irc_server
cargo run

# Terminal 2: run the backend pointed at the spoof server
cd server
KOLMODIN__TWITCH__IRC_SERVER_URL=localhost:6667 cargo run --release
```

If `KOLMODIN__TWITCH__IRC_SERVER_URL` is unset, the server connects to the real Twitch endpoint (`irc.chat.twitch.tv:6667`).

### Creating Custom Questions

The server expects a json file with whitelisted channels and question data. See `server/kolmodin_data_example.json` for the correct structure. The server can load this file from file or through HTTP URL. Use `GET /api/refresh-words` with the admin API key to reload updated data without restarting.
