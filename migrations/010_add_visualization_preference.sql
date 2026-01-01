-- Add preferred_visualization column to contract_queries table
ALTER TABLE contract_queries ADD COLUMN preferred_visualization TEXT DEFAULT 'table' CHECK(preferred_visualization IN ('bar', 'line', 'pie', 'table', 'number'));

-- Add index for better query performance
CREATE INDEX IF NOT EXISTS idx_contract_queries_visualization ON contract_queries(preferred_visualization);
