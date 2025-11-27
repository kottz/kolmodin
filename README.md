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

### Creating Custom Questions

The server expects a json file with whitelisted channels and question data. See `server/kolmodin_data_example.json` for the correct structure. The server can load this file from file or through HTTP URL. Use `GET /api/refresh-words` with the admin API key to reload updated data without restarting.
