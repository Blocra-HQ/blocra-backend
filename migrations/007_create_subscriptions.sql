-- Subscription plans table
CREATE TABLE IF NOT EXISTS subscription_plans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    price_usdt REAL NOT NULL,
    report_limit INTEGER NOT NULL,
    duration_days INTEGER NOT NULL,
    description TEXT,
    is_active BOOLEAN DEFAULT 1,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Supported chains for payments
CREATE TABLE IF NOT EXISTS payment_chains (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chain_id TEXT NOT NULL UNIQUE,
    chain_name TEXT NOT NULL,
    usdt_contract_address TEXT NOT NULL,
    is_active BOOLEAN DEFAULT 1,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- User subscriptions table
CREATE TABLE IF NOT EXISTS user_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    wallet_address TEXT NOT NULL,
    plan_id INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active', 'cancelled', 'expired', 'pending')),
    reports_used INTEGER DEFAULT 0,
    reports_limit INTEGER NOT NULL,
    start_date TIMESTAMP NOT NULL,
    end_date TIMESTAMP NOT NULL,
    payment_chain_id TEXT,
    tx_hash TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (plan_id) REFERENCES subscription_plans(id)
);

-- Payment transactions table
CREATE TABLE IF NOT EXISTS payment_transactions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    wallet_address TEXT NOT NULL,
    amount_usdt REAL NOT NULL,
    payment_type TEXT NOT NULL CHECK(payment_type IN ('one_time', 'subscription')),
    status TEXT NOT NULL CHECK(status IN ('pending', 'completed', 'failed', 'refunded')),
    payment_chain_id TEXT NOT NULL,
    tx_hash TEXT,
    subscription_id INTEGER,
    metadata TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (subscription_id) REFERENCES user_subscriptions(id)
);

-- Report usage tracking table
CREATE TABLE IF NOT EXISTS report_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    contract_address TEXT NOT NULL,
    report_type TEXT DEFAULT 'eda',
    payment_type TEXT NOT NULL CHECK(payment_type IN ('one_time', 'subscription', 'admin_free')),
    subscription_id INTEGER,
    transaction_id INTEGER,
    is_admin_generated BOOLEAN DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (subscription_id) REFERENCES user_subscriptions(id),
    FOREIGN KEY (transaction_id) REFERENCES payment_transactions(id)
);

-- Insert default plans (prices in USDT)
INSERT INTO subscription_plans (name, price_usdt, report_limit, duration_days, description) VALUES
('Pay Per Report', 3.0, 1, 0, 'One-time payment for a single EDA report - 3 USDT'),
('Monthly Pro', 50.0, 100, 30, 'Monthly subscription with 100 EDA reports - 50 USDT/month');

-- Insert supported chains with USDT contract addresses
INSERT INTO payment_chains (chain_id, chain_name, usdt_contract_address) VALUES
('ethereum', 'Ethereum', '0xdAC17F958D2ee523a2206206994597C13D831ec7'),
('polygon', 'Polygon', '0xc2132D05D31c914a87C6611C10748AEb04B58e8F'),
('avalanche', 'Avalanche', '0x9702230A8Ea53601f5cD2dc00fDBc13d4dF4A8c7'),
('bnb', 'BNB Smart Chain', '0x55d398326f99059fF775485246999027B3197955'),
('arbitrum', 'Arbitrum', '0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9'),
('optimism', 'Optimism', '0x94b008aA00579c1307B0EF2c499aD98a8ce58e58'),
('base', 'Base', '0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2');

-- Create indexes for performance
CREATE INDEX idx_user_subscriptions_user_id ON user_subscriptions(user_id);
CREATE INDEX idx_user_subscriptions_wallet ON user_subscriptions(wallet_address);
CREATE INDEX idx_user_subscriptions_status ON user_subscriptions(status);
CREATE INDEX idx_payment_transactions_user_id ON payment_transactions(user_id);
CREATE INDEX idx_payment_transactions_wallet ON payment_transactions(wallet_address);
CREATE INDEX idx_payment_transactions_status ON payment_transactions(status);
CREATE INDEX idx_payment_transactions_tx_hash ON payment_transactions(tx_hash);
CREATE INDEX idx_report_usage_user_id ON report_usage(user_id);
CREATE INDEX idx_report_usage_subscription_id ON report_usage(subscription_id);
CREATE INDEX idx_report_usage_admin ON report_usage(is_admin_generated);

