-- Create contract_queries table
CREATE TABLE IF NOT EXISTS contract_queries (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    contract_address TEXT NOT NULL,
    query_type TEXT NOT NULL,
    parameters TEXT,
    result TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_queries_user ON contract_queries(user_id);
CREATE INDEX idx_queries_contract ON contract_queries(contract_address);
CREATE INDEX idx_queries_status ON contract_queries(status);
