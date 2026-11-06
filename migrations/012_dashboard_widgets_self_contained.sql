-- Dashboard widgets used to point at contract_queries(id), but that table
-- stores contract-analysis requests (TEXT ids, no SQL), so no widget could
-- ever be saved. Widgets now carry what they show themselves:
--   query_sql  - the SQL the builder re-runs with the in-browser query engine
--   data_json  - a JSON snapshot of rows (e.g. figures from Contract Analysis)
-- At least one of the two must be present. query_id stays as an optional,
-- unenforced reference for older rows.
CREATE TABLE dashboard_widgets_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dashboard_id INTEGER NOT NULL,
    query_id INTEGER,
    query_sql TEXT,
    data_json TEXT,
    title TEXT NOT NULL,
    visualization_type TEXT NOT NULL CHECK(visualization_type IN ('bar', 'line', 'pie', 'table', 'number')),
    position_x INTEGER NOT NULL DEFAULT 0,
    position_y INTEGER NOT NULL DEFAULT 0,
    width INTEGER NOT NULL DEFAULT 400,
    height INTEGER NOT NULL DEFAULT 300,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (dashboard_id) REFERENCES dashboards(id) ON DELETE CASCADE,
    CHECK (query_sql IS NOT NULL OR data_json IS NOT NULL OR query_id IS NOT NULL)
);

INSERT INTO dashboard_widgets_new
    (id, dashboard_id, query_id, title, visualization_type, position_x, position_y, width, height, created_at, updated_at)
SELECT id, dashboard_id, query_id, title, visualization_type, position_x, position_y, width, height, created_at, updated_at
FROM dashboard_widgets;

DROP TABLE dashboard_widgets;
ALTER TABLE dashboard_widgets_new RENAME TO dashboard_widgets;

CREATE INDEX IF NOT EXISTS idx_dashboard_widgets_dashboard_id ON dashboard_widgets(dashboard_id);
