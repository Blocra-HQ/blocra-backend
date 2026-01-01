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
    pub query_id: i64,
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

#[derive(Debug, Deserialize)]
pub struct CreateDashboardPayload {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWidgetPayload {
    pub query_id: i64,
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
    pub widgets: Vec<WidgetWithQuery>,
}

#[derive(Debug, Serialize)]
pub struct WidgetWithQuery {
    pub widget: DashboardWidget,
    pub query_name: String,
    pub query_content: String,
}

#[derive(Debug, Serialize)]
pub struct ChartSuggestion {
    pub visualization_type: String,
    pub confidence: f64,
    pub reason: String,
}
