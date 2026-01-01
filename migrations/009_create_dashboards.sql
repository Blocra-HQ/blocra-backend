-- Create dashboards table
CREATE TABLE IF NOT EXISTS dashboards (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Create dashboard_widgets table
CREATE TABLE IF NOT EXISTS dashboard_widgets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dashboard_id INTEGER NOT NULL,
    query_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    visualization_type TEXT NOT NULL CHECK(visualization_type IN ('bar', 'line', 'pie', 'table', 'number')),
    position_x INTEGER NOT NULL DEFAULT 0,
    position_y INTEGER NOT NULL DEFAULT 0,
    width INTEGER NOT NULL DEFAULT 400,
    height INTEGER NOT NULL DEFAULT 300,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (dashboard_id) REFERENCES dashboards(id) ON DELETE CASCADE,
    FOREIGN KEY (query_id) REFERENCES contract_queries(id) ON DELETE CASCADE
);

-- Create widget_preferences table for AI learning
CREATE TABLE IF NOT EXISTS widget_preferences (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    query_id INTEGER NOT NULL,
    visualization_type TEXT NOT NULL,
    selection_count INTEGER DEFAULT 1,
    last_selected_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (query_id) REFERENCES contract_queries(id) ON DELETE CASCADE,
    UNIQUE(user_id, query_id, visualization_type)
);

-- Create indexes for performance
CREATE INDEX IF NOT EXISTS idx_dashboards_user_id ON dashboards(user_id);
CREATE INDEX IF NOT EXISTS idx_dashboard_widgets_dashboard_id ON dashboard_widgets(dashboard_id);
CREATE INDEX IF NOT EXISTS idx_dashboard_widgets_query_id ON dashboard_widgets(query_id);
CREATE INDEX IF NOT EXISTS idx_widget_preferences_user_id ON widget_preferences(user_id);
CREATE INDEX IF NOT EXISTS idx_widget_preferences_query_id ON widget_preferences(query_id);
