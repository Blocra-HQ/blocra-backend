-- Activity logging for admin monitoring
-- Tracks all user actions: logins, queries, reports, contract interactions

CREATE TABLE IF NOT EXISTS activity_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    activity_type TEXT NOT NULL, -- 'login', 'query', 'report', 'contract_view', 'api_call'
    description TEXT NOT NULL,
    metadata TEXT, -- JSON with additional data
    ip_address TEXT,
    user_agent TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Indexes for fast admin queries
CREATE INDEX IF NOT EXISTS idx_activity_user_id ON activity_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_activity_type ON activity_logs(activity_type);
CREATE INDEX IF NOT EXISTS idx_activity_created_at ON activity_logs(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_activity_user_type ON activity_logs(user_id, activity_type);

-- Query execution logs (detailed tracking)
CREATE TABLE IF NOT EXISTS query_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    query_text TEXT NOT NULL,
    query_type TEXT, -- 'sql', 'contract_analysis', 'event_fetch'
    contract_address TEXT,
    execution_time_ms INTEGER,
    result_count INTEGER,
    success BOOLEAN DEFAULT TRUE,
    error_message TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_query_logs_user_id ON query_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_query_logs_contract ON query_logs(contract_address);
CREATE INDEX IF NOT EXISTS idx_query_logs_created_at ON query_logs(created_at DESC);

-- Report generation logs
CREATE TABLE IF NOT EXISTS report_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    report_type TEXT NOT NULL, -- 'dashboard', 'contract_analysis', 'event_summary'
    report_name TEXT,
    contract_addresses TEXT, -- JSON array
    parameters TEXT, -- JSON with report parameters
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_report_logs_user_id ON report_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_report_logs_created_at ON report_logs(created_at DESC);
