-- Payments table for tips and future paid features
-- Supports BTCPay Server integration

CREATE TABLE IF NOT EXISTS payments (
    id BIGSERIAL PRIMARY KEY,
    
    -- BTCPay invoice reference
    btcpay_invoice_id TEXT NOT NULL UNIQUE,
    
    -- Who paid (NULL for anonymous tips)
    user_npub TEXT REFERENCES users(npub) ON DELETE SET NULL,
    
    -- Payment type: 'tip', 'quota_increase', 'storage', etc.
    payment_type TEXT NOT NULL DEFAULT 'tip',
    
    -- Amount in satoshis
    amount_sats BIGINT NOT NULL,
    
    -- Optional message (for tips)
    message TEXT,
    
    -- Payment status: 'pending', 'paid', 'expired', 'invalid'
    status TEXT NOT NULL DEFAULT 'pending',
    
    -- Payment method used: 'lightning', 'onchain' (filled after payment)
    payment_method TEXT,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    paid_at TIMESTAMPTZ
);

-- Index for webhook lookups (find payment by BTCPay invoice ID)
CREATE INDEX IF NOT EXISTS idx_payments_btcpay_invoice ON payments(btcpay_invoice_id);

-- Index for user payment history
CREATE INDEX IF NOT EXISTS idx_payments_user ON payments(user_npub, created_at DESC);

-- Index for admin view (recent paid payments)
CREATE INDEX IF NOT EXISTS idx_payments_paid ON payments(paid_at DESC) WHERE status = 'paid';

-- Index for cleanup of expired payments
CREATE INDEX IF NOT EXISTS idx_payments_status ON payments(status, created_at);
