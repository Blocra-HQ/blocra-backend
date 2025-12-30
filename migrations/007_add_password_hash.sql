-- Add password_hash column to users table for secure password storage
-- This migration adds support for password-based authentication

-- Add password_hash column (nullable to support OAuth-only accounts)
ALTER TABLE users ADD COLUMN password_hash TEXT;

-- Create index for faster email lookups during login
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
