use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Dashboard {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DashboardWidget {
    pub id: i64,
    pub dashboard_id: i64,
    pub query_id: Option<i64>,
    /// SQL the builder re-runs with the in-browser query engine.
    pub query_sql: Option<String>,
    /// JSON snapshot of rows, for widgets that are not backed by a query.
    pub data_json: Option<String>,
    pub title: String,
    pub visualization_type: String,
    pub position_x: i64,
    pub position_y: i64,
    pub width: i64,
    pub height: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WidgetPreference {
    pub id: i64,
    pub user_id: i64,
    pub query_id: i64,
    pub visualization_type: String,
    pub selection_count: i64,
    pub last_selected_at: DateTime<Utc>,
}

/// Body of POST /api/dashboard-builder and PUT /api/dashboard-builder/{id}.
/// On PUT, `widgets` (when given) replaces every widget of the dashboard.
#[derive(Debug, Deserialize)]
pub struct CreateDashboardPayload {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub widgets: Option<Vec<CreateWidgetPayload>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWidgetPayload {
    #[serde(default)]
    pub query_id: Option<i64>,
    #[serde(default)]
    pub query_sql: Option<String>,
    /// Any JSON value; stored as text.
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    pub title: String,
    pub visualization_type: String,
    pub position_x: i64,
    pub position_y: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Serialize)]
pub struct DashboardWithWidgets {
    pub dashboard: Dashboard,
    pub widgets: Vec<DashboardWidget>,
}

#[derive(Debug, Serialize)]
pub struct ChartSuggestion {
    pub visualization_type: String,
    pub confidence: f64,
    pub reason: String,
}
