-- Create bounties table
CREATE TABLE IF NOT EXISTS bounties (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    reward_amount REAL NOT NULL,
    reward_token TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    difficulty TEXT NOT NULL,
    category TEXT NOT NULL,
    created_by TEXT NOT NULL,
    deadline TIMESTAMP,
    max_participants INTEGER,
    requirements TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_bounties_status ON bounties(status);
CREATE INDEX idx_bounties_created_by ON bounties(created_by);
CREATE INDEX idx_bounties_category ON bounties(category);
