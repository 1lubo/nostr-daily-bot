# BTCPay Tipping - Implementation Plan

**Design:** [2026-04-21-btcpay-tipping-design.md](./2026-04-21-btcpay-tipping-design.md)  
**Branch:** `feature/btcpay-tipping`

## Phase 1: Database & Configuration

### Task 1.1: Create payments migration
- [ ] Create `migrations/003_payments.sql`
- [ ] Add payments table with all columns
- [ ] Add indexes

### Task 1.2: Add BTCPay configuration
- [ ] Create `src/config/btcpay.rs` with `BTCPayConfig` struct
- [ ] Load from environment variables
- [ ] Update `src/config/mod.rs` to export
- [ ] Update `.env.example` with BTCPay vars

## Phase 2: BTCPay Client Module

### Task 2.1: Create BTCPay error types
- [ ] Create `src/btcpay/error.rs` with `BTCPayError` enum
- [ ] Variants: ApiError, NetworkError, InvalidResponse, WebhookVerificationFailed

### Task 2.2: Create BTCPay client
- [ ] Create `src/btcpay/client.rs` with `BTCPayClient` struct
- [ ] Implement `new()` from config
- [ ] Implement `create_invoice()` method
- [ ] Implement `get_invoice()` method (for status checking)

### Task 2.3: Implement webhook verification
- [ ] Create `src/btcpay/webhook.rs`
- [ ] Implement HMAC-SHA256 signature verification
- [ ] Parse webhook payload types

### Task 2.4: Create module structure
- [ ] Create `src/btcpay/mod.rs`
- [ ] Export client, error, webhook
- [ ] Add `mod btcpay` to `src/main.rs`

## Phase 3: Database Operations

### Task 3.1: Create payments database module
- [ ] Create `src/db/payments.rs`
- [ ] Implement `create_payment()` - insert pending payment
- [ ] Implement `get_payment_by_invoice_id()` - for webhook lookup
- [ ] Implement `update_payment_status()` - mark paid/expired
- [ ] Implement `list_payments()` - for admin view

### Task 3.2: Update database module exports
- [ ] Add `pub mod payments` to `src/db/mod.rs`

## Phase 4: API Endpoints

### Task 4.1: Add request/response types
- [ ] Add `CreateTipRequest` struct (amount_sats, message, token)
- [ ] Add `CreateTipResponse` struct (invoice_id, checkout_url, amount_sats)
- [ ] Add `WebhookPayload` struct (BTCPay webhook format)
- [ ] Add `PaymentStatusResponse` struct
- [ ] Add `AdminPaymentsResponse` struct

### Task 4.2: Implement tip creation endpoint
- [ ] Add `create_tip` handler in `src/api/handlers.rs`
- [ ] Validate amount (min/max bounds)
- [ ] Call BTCPayClient to create invoice
- [ ] Store pending payment in DB
- [ ] Return checkout URL

### Task 4.3: Implement webhook endpoint
- [ ] Add `tip_webhook` handler
- [ ] Verify signature from BTCPay-Sig header
- [ ] Parse event type (InvoiceSettled, InvoiceExpired, etc.)
- [ ] Update payment status in DB
- [ ] Return 200 OK

### Task 4.4: Implement status endpoint
- [ ] Add `tip_status` handler
- [ ] Look up payment by invoice_id
- [ ] Return current status

### Task 4.5: Implement admin payments endpoint
- [ ] Add `admin_payments` handler
- [ ] Require admin token (new env var: ADMIN_TOKEN)
- [ ] Return paginated payment list

### Task 4.6: Register routes
- [ ] Add routes to `src/api/routes.rs`

## Phase 5: Application State

### Task 5.1: Add BTCPay client to AppState
- [ ] Add `btcpay: Option<BTCPayClient>` to `AppState`
- [ ] Initialize in `run_server()` if config present
- [ ] Log whether tipping is enabled

## Phase 6: Frontend Integration

### Task 6.1: Add BTCPay modal script
- [ ] Add BTCPay JS script tag to `static/index.html`
- [ ] Make script URL configurable via API response

### Task 6.2: Add tip creation function
- [ ] Add `createTip(useModal)` JavaScript function
- [ ] Handle both modal and redirect modes
- [ ] Show loading state during API call

### Task 6.3: Add dedicated tip section
- [ ] Add tip form with amount input (default 5000)
- [ ] Add optional message textarea
- [ ] Add "Pay with Lightning" button (modal)
- [ ] Add "Pay On-chain" button (redirect)

### Task 6.4: Add embedded tip widget
- [ ] Add small "Support this bot" button in footer
- [ ] Opens tip modal with default amount

### Task 6.5: Add post-action tip prompt
- [ ] After successful quote save, show tip suggestion
- [ ] Non-intrusive, dismissible

### Task 6.6: Add success page handling
- [ ] Handle `/tip/success` route
- [ ] Show thank you message
- [ ] Poll for payment confirmation if needed

## Phase 7: Testing

### Task 7.1: Unit tests
- [ ] Test BTCPayConfig loading
- [ ] Test webhook signature verification
- [ ] Test payment database operations

### Task 7.2: Integration tests
- [ ] Test tip creation endpoint (mock BTCPay)
- [ ] Test webhook processing
- [ ] Test status endpoint

## Phase 8: Documentation

### Task 8.1: Update README
- [ ] Add tipping section
- [ ] Document BTCPay setup requirements
- [ ] Document environment variables

### Task 8.2: Update .env.example
- [ ] Add all BTCPay environment variables with comments

## Verification Checklist

- [ ] `cargo build` succeeds
- [ ] `cargo test` passes
- [ ] `cargo clippy` has no warnings
- [ ] Tipping works end-to-end with BTCPay demo server
- [ ] Webhook updates payment status correctly
- [ ] UI shows/hides based on BTCPay configuration
