# BTCPay Server Tipping Integration - Design Document

**Date:** 2026-04-21  
**Status:** Approved  
**Branch:** `feature/btcpay-tipping`

## Overview

Add Bitcoin/Lightning tipping functionality to Nostr Daily Bot using BTCPay Server's Greenfield API. Tips support the bot service itself.

## Requirements

| Requirement | Decision |
|-------------|----------|
| Recipient | Bot itself (single recipient) |
| Touchpoints | Dedicated page, embedded widget, after actions |
| Frontend | Embedded in backend (current approach) |
| Payment UI | Both modal + redirect options |
| Amounts | Suggested default (5,000 sats) with custom input |
| After payment | Thank you page + optional message from tipper |
| Message visibility | Private (admin only) |
| Currency display | Sats only |
| BTCPay instance | Demo server for development |
| Future expansion | Schema supports paid features (quota, storage) |

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         User clicks "Tip"                           │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│  Bot Backend (Rust/Axum)                                            │
│  POST /api/tips/create                                              │
│  - Calls BTCPay Greenfield API to create invoice                    │
│  - Stores pending tip in PostgreSQL                                 │
│  - Returns invoice ID + checkout URL                                │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                    ┌──────────────┴──────────────┐
                    ▼                              ▼
             [Modal Mode]                   [Redirect Mode]
          BTCPay JS modal                  Redirect to BTCPay
          opens on your site               checkout page
                    │                              │
                    └──────────────┬──────────────┘
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│  User pays via Lightning or On-chain                                │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│  BTCPay Server sends webhook                                        │
│  POST /api/tips/webhook                                             │
│  - Verify webhook signature                                         │
│  - Update payment status in DB (pending → paid)                     │
│  - Store tipper's optional message                                  │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│  User redirected to /tip/success?id=xxx                             │
│  - Shows "Thank you!" message                                       │
└─────────────────────────────────────────────────────────────────────┘
```

## Database Schema

```sql
CREATE TABLE IF NOT EXISTS payments (
    id BIGSERIAL PRIMARY KEY,
    btcpay_invoice_id TEXT NOT NULL UNIQUE,
    user_npub TEXT REFERENCES users(npub) ON DELETE SET NULL,
    payment_type TEXT NOT NULL DEFAULT 'tip',
    amount_sats BIGINT NOT NULL,
    message TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    payment_method TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    paid_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_payments_btcpay_invoice ON payments(btcpay_invoice_id);
CREATE INDEX IF NOT EXISTS idx_payments_user ON payments(user_npub, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_payments_paid ON payments(paid_at DESC) WHERE status = 'paid';
```

## API Endpoints

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `/api/tips/create` | POST | Optional | Create BTCPay invoice |
| `/api/tips/webhook` | POST | BTCPay signature | Receive payment notifications |
| `/api/tips/status/{invoice_id}` | GET | None | Check payment status |
| `/api/admin/payments` | GET | Admin token | List all payments |

## Configuration

Environment variables:
- `BTCPAY_BASE_URL` - BTCPay server URL
- `BTCPAY_API_KEY` - API key with invoice permissions
- `BTCPAY_STORE_ID` - Store ID
- `BTCPAY_WEBHOOK_SECRET` - For webhook signature verification

## File Structure

```
src/
├── btcpay/
│   ├── mod.rs          # BTCPayClient struct
│   ├── client.rs       # API calls
│   ├── webhook.rs      # Signature verification
│   └── error.rs        # BTCPayError enum
├── config/
│   └── btcpay.rs       # BTCPayConfig from env
├── api/
│   └── handlers.rs     # + tip endpoints
└── db/
    └── payments.rs     # Payment CRUD operations

migrations/
└── 003_payments.sql

static/
└── index.html          # + tipping UI elements
```

## Error Handling

| Scenario | Behavior |
|----------|----------|
| BTCPay API unreachable | Return 503, user sees "Payment service unavailable" |
| Invalid webhook signature | Return 401, ignore payload |
| Invoice already processed | Return 200 (idempotent) |
| User's tip expired | Update status to `expired` |
| Missing env vars at startup | Fail fast with clear error |

## Graceful Degradation

If BTCPay is not configured, the bot works normally - tipping UI is hidden.
