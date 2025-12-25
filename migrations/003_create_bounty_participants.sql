-- Create bounty_participants table
CREATE TABLE IF NOT EXISTS bounty_participants (
    id TEXT PRIMARY KEY,
    bounty_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    joined_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (bounty_id) REFERENCES bounties(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    UNIQUE(bounty_id, user_id)
);

CREATE INDEX idx_participants_bounty ON bounty_participants(bounty_id);
CREATE INDEX idx_participants_user ON bounty_participants(user_id);
