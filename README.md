# Nostr Daily Bot

A multi-user bot that posts scheduled messages to Nostr relays. Authenticate securely with your browser extension (NIP-07) or use nsec fallback.

## Features

- 🔐 **NIP-07 Authentication** - Sign with browser extension (nos2x, Alby). Private key never leaves your device.
- 📅 **Pre-signed Scheduling** - Sign a batch of posts locally, server publishes them on schedule
- ⏰ **Cron Scheduling** - Flexible scheduling with cron expressions
- 🌐 **Web UI** - Simple interface for managing quotes and schedules
- 🔄 **Multi-user** - Each user has their own quotes, schedule, and history
- ⚡ **Bitcoin Tipping** - Accept tips via Lightning or on-chain using BTCPay Server

## Quick Start

### Option 1: Fly.io (Recommended)

```bash
fly launch
fly postgres create --name nostr-bot-db
fly postgres attach nostr-bot-db
fly deploy
```

### Option 2: Docker

```bash
docker run -d --name postgres -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=nostr_bot -p 5432:5432 postgres:15
docker build -t nostr-daily-bot .
docker run -p 3000:3000 -e DATABASE_URL=postgres://postgres:postgres@host.docker.internal/nostr_bot nostr-daily-bot
```

### Option 3: Local Development

```bash
# Start PostgreSQL (e.g., via Docker)
docker run -d --name postgres -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=nostr_bot -p 5432:5432 postgres:15

# Run the bot
export DATABASE_URL="postgres://postgres:postgres@localhost/nostr_bot"
cargo run -- serve --port 3000
```

Open http://localhost:3000 in your browser.

## Configuration

### Core Settings

| Variable | Required | Description |
|----------|----------|-------------|
| `DATABASE_URL` | Yes | PostgreSQL connection string |
| `RUST_LOG` | No | Log level: error, warn, info (default), debug, trace |
| `PUBLIC_URL` | No | Base URL for redirects (e.g., `https://yourdomain.com`) |

### BTCPay Server Integration (Optional)

Enable Bitcoin/Lightning tipping by connecting to a BTCPay Server instance:

| Variable | Required | Description |
|----------|----------|-------------|
| `BTCPAY_BASE_URL` | For tipping | BTCPay Server URL (e.g., `https://mainnet.demo.btcpayserver.org`) |
| `BTCPAY_API_KEY` | For tipping | API key with invoice permissions |
| `BTCPAY_STORE_ID` | For tipping | Store ID from BTCPay |
| `BTCPAY_WEBHOOK_SECRET` | For tipping | Webhook secret for signature verification |
| `BTCPAY_DEFAULT_TIP_SATS` | No | Default tip amount (default: 5000 sats) |
| `ADMIN_TOKEN` | No | Token to access `/api/admin/payments` endpoint |

**Setup steps:**
1. Create a store in BTCPay Server
2. Generate an API key: Account → API Keys → Create (enable invoice permissions)
3. Create a webhook: Store → Settings → Webhooks → Create
   - URL: `https://yourdomain.com/api/tips/webhook`
   - Events: `InvoiceSettled`, `InvoiceExpired`, `InvoiceInvalid`
4. Copy the webhook secret and set `BTCPAY_WEBHOOK_SECRET`

## Usage

1. **Login** - Click "Login with Extension" (recommended) or enter your nsec
2. **Add Quotes** - Enter messages to post, one per line
3. **Set Schedule** - Configure cron expression (e.g., `0 0 9 * * *` for daily at 9 AM UTC)
4. **Sign Posts** - If using NIP-07, sign the batch of upcoming posts
5. **Done!** - The bot posts your messages on schedule

## Cron Examples

| Expression | Description |
|------------|-------------|
| `0 0 9 * * *` | Daily at 9:00 AM UTC |
| `0 0 */6 * * *` | Every 6 hours |
| `0 30 8 * * 1-5` | Weekdays at 8:30 AM UTC |
| `0 0 12,18 * * *` | Twice daily at noon and 6 PM UTC |

## API

See [SPEC.md](SPEC.md) for full API documentation.

## License

MIT

