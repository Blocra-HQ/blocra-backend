use crate::{db::DbPool, models::dashboard::*, utils::jwt, errors::AppError};
use actix_web::HttpRequest;
use serde_json::json;
use sqlx::Row;

pub async fn create_dashboard(
    pool: &DbPool,
    req: &HttpRequest,
    payload: CreateDashboardPayload,
) -> Result<Dashboard, AppError> {
    let user_id = jwt::extract_user_id(req)?;

    let dashboard = sqlx::query_as::<_, Dashboard>(
        "INSERT INTO dashboards (user_id, name, description) VALUES (?, ?, ?)
         RETURNING *"
    )
    .bind(user_id)
    .bind(&payload.name)
    .bind(&payload.description)
    .fetch_one(pool)
    .await?;

    Ok(dashboard)
}

pub async fn get_user_dashboards(
    pool: &DbPool,
    req: &HttpRequest,
) -> Result<Vec<Dashboard>, AppError> {
    let user_id = jwt::extract_user_id(req)?;

    let dashboards = sqlx::query_as::<_, Dashboard>(
        "SELECT * FROM dashboards WHERE user_id = ? ORDER BY updated_at DESC"
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(dashboards)
}

pub async fn get_dashboard_with_widgets(
    pool: &DbPool,
    req: &HttpRequest,
    dashboard_id: i64,
) -> Result<DashboardWithWidgets, AppError> {
    let user_id = jwt::extract_user_id(req)?;

    // Get dashboard
    let dashboard = sqlx::query_as::<_, Dashboard>(
        "SELECT * FROM dashboards WHERE id = ? AND user_id = ?"
    )
    .bind(dashboard_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    // Get widgets with query info
    #[derive(sqlx::FromRow)]
    struct WidgetQueryRow {
        id: i64,
        dashboard_id: i64,
        query_id: i64,
        title: String,
        visualization_type: String,
        position_x: i64,
        position_y: i64,
        width: i64,
        height: i64,
        created_at: chrono::DateTime<chrono::Utc>,
        updated_at: chrono::DateTime<chrono::Utc>,
        query_name: String,
        query_content: String,
    }

    let rows = sqlx::query_as::<_, WidgetQueryRow>(
        "SELECT w.*, q.name as query_name, q.query as query_content
         FROM dashboard_widgets w
         JOIN contract_queries q ON w.query_id = q.id
         WHERE w.dashboard_id = ?
         ORDER BY w.created_at"
    )
    .bind(dashboard_id)
    .fetch_all(pool)
    .await?;

    let widgets_with_query: Vec<WidgetWithQuery> = rows
        .into_iter()
        .map(|row| WidgetWithQuery {
            widget: DashboardWidget {
                id: row.id,
                dashboard_id: row.dashboard_id,
                query_id: row.query_id,
                title: row.title,
                visualization_type: row.visualization_type,
                position_x: row.position_x,
                position_y: row.position_y,
                width: row.width,
                height: row.height,
                created_at: row.created_at,
                updated_at: row.updated_at,
            },
            query_name: row.query_name,
            query_content: row.query_content,
        })
        .collect();

    Ok(DashboardWithWidgets {
        dashboard,
        widgets: widgets_with_query,
    })
}

pub async fn add_widget_to_dashboard(
    pool: &DbPool,
    req: &HttpRequest,
    dashboard_id: i64,
    payload: CreateWidgetPayload,
) -> Result<DashboardWidget, AppError> {
    let user_id = jwt::extract_user_id(req)?;

    // Verify dashboard ownership
    let _dashboard = sqlx::query_as::<_, Dashboard>(
        "SELECT * FROM dashboards WHERE id = ? AND user_id = ?"
    )
    .bind(dashboard_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    // Create widget
    let widget = sqlx::query_as::<_, DashboardWidget>(
        "INSERT INTO dashboard_widgets 
         (dashboard_id, query_id, title, visualization_type, position_x, position_y, width, height)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING *"
    )
    .bind(dashboard_id)
    .bind(payload.query_id)
    .bind(&payload.title)
    .bind(&payload.visualization_type)
    .bind(payload.position_x)
    .bind(payload.position_y)
    .bind(payload.width)
    .bind(payload.height)
    .fetch_one(pool)
    .await?;

    // Record preference for AI learning
    sqlx::query(
        "INSERT INTO widget_preferences (user_id, query_id, visualization_type, selection_count, last_selected_at)
         VALUES (?, ?, ?, 1, CURRENT_TIMESTAMP)
         ON CONFLICT(user_id, query_id, visualization_type) 
         DO UPDATE SET 
            selection_count = selection_count + 1,
            last_selected_at = CURRENT_TIMESTAMP"
    )
    .bind(user_id)
    .bind(payload.query_id)
    .bind(&payload.visualization_type)
    .execute(pool)
    .await?;

    // Update dashboard timestamp
    sqlx::query("UPDATE dashboards SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(dashboard_id)
        .execute(pool)
        .await?;

    Ok(widget)
}

pub async fn get_saved_queries(
    pool: &DbPool,
    req: &HttpRequest,
) -> Result<Vec<serde_json::Value>, AppError> {
    let user_id = jwt::extract_user_id(req)?;

    let queries = sqlx::query(
        "SELECT id, name, query, contract_address, created_at 
         FROM contract_queries 
         WHERE user_id = ? 
         ORDER BY created_at DESC"
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let result: Vec<serde_json::Value> = queries
        .iter()
        .map(|row| {
            json!({
                "id": row.get::<i64, _>("id"),
                "name": row.get::<String, _>("name"),
                "query": row.get::<String, _>("query"),
                "contract_address": row.get::<Option<String>, _>("contract_address"),
                "created_at": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        })
        .collect();

    Ok(result)
}

pub async fn suggest_chart_type(
    pool: &DbPool,
    req: &HttpRequest,
    query_id: i64,
) -> Result<Vec<ChartSuggestion>, AppError> {
    let user_id = jwt::extract_user_id(req)?;

    // Get user's preference history for this query
    let preferences = sqlx::query_as::<_, WidgetPreference>(
        "SELECT * FROM widget_preferences 
         WHERE user_id = ? AND query_id = ?
         ORDER BY selection_count DESC, last_selected_at DESC"
    )
    .bind(user_id)
    .bind(query_id)
    .fetch_all(pool)
    .await?;

    // Get total selections for this user
    let total_selections: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(selection_count), 0) FROM widget_preferences WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let mut suggestions = Vec::new();

    // Only suggest if user has made at least 2 selections
    if total_selections >= 2 {
        if let Some(top_pref) = preferences.first() {
            suggestions.push(ChartSuggestion {
                visualization_type: top_pref.visualization_type.clone(),
                confidence: (top_pref.selection_count as f64 / total_selections as f64) * 100.0,
                reason: format!(
                    "You've used {} {} time(s) for similar queries",
                    top_pref.visualization_type, top_pref.selection_count
                ),
            });
        }

        // Get global preferences for similar queries
        let global_prefs = sqlx::query_as::<_, (String, i64)>(
            "SELECT visualization_type, SUM(selection_count) as total
             FROM widget_preferences
             WHERE user_id = ?
             GROUP BY visualization_type
             ORDER BY total DESC
             LIMIT 3"
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        for (viz_type, count) in global_prefs {
            if !suggestions.iter().any(|s| s.visualization_type == viz_type) {
                suggestions.push(ChartSuggestion {
                    visualization_type: viz_type.clone(),
                    confidence: (count as f64 / total_selections as f64) * 100.0,
                    reason: format!("You frequently use {} charts", viz_type),
                });
            }
        }
    }

    Ok(suggestions)
}

pub async fn update_widget_position(
    pool: &DbPool,
    req: &HttpRequest,
    widget_id: i64,
    position_x: i64,
    position_y: i64,
    width: i64,
    height: i64,
) -> Result<DashboardWidget, AppError> {
    let user_id = jwt::extract_user_id(req)?;

    // Verify ownership through dashboard
    let widget = sqlx::query_as::<_, DashboardWidget>(
        "UPDATE dashboard_widgets 
         SET position_x = ?, position_y = ?, width = ?, height = ?, updated_at = CURRENT_TIMESTAMP
         WHERE id = ? AND dashboard_id IN (SELECT id FROM dashboards WHERE user_id = ?)
         RETURNING *"
    )
    .bind(position_x)
    .bind(position_y)
    .bind(width)
    .bind(height)
    .bind(widget_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(widget)
}

pub async fn delete_widget(
    pool: &DbPool,
    req: &HttpRequest,
    widget_id: i64,
) -> Result<(), AppError> {
    let user_id = jwt::extract_user_id(req)?;

    sqlx::query(
        "DELETE FROM dashboard_widgets 
         WHERE id = ? AND dashboard_id IN (SELECT id FROM dashboards WHERE user_id = ?)"
    )
    .bind(widget_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete_dashboard(
    pool: &DbPool,
    req: &HttpRequest,
    dashboard_id: i64,
) -> Result<(), AppError> {
    let user_id = jwt::extract_user_id(req)?;

    sqlx::query("DELETE FROM dashboards WHERE id = ? AND user_id = ?")
        .bind(dashboard_id)
        .bind(user_id)
        .execute(pool)
        .await?;

    Ok(())
}
