use crate::{db::DbPool, models::dashboard::*, utils::jwt, errors::AppError};
use actix_web::HttpRequest;

const VISUALIZATION_TYPES: [&str; 5] = ["bar", "line", "pie", "table", "number"];
const MAX_NAME_LEN: usize = 200;
const MAX_WIDGETS: usize = 50;
const MAX_SQL_LEN: usize = 20_000;
const MAX_DATA_BYTES: usize = 512 * 1024;

/// A widget ready to insert: validated, with its snapshot serialised.
struct NewWidget<'a> {
    query_id: Option<i64>,
    query_sql: Option<&'a str>,
    data_json: Option<String>,
    payload: &'a CreateWidgetPayload,
}

fn validate_name(name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("Dashboard name is required".into()));
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(AppError::BadRequest(format!("Dashboard name is longer than {MAX_NAME_LEN} characters")));
    }
    Ok(())
}

fn validate_widget(payload: &CreateWidgetPayload) -> Result<NewWidget<'_>, AppError> {
    if payload.title.trim().is_empty() || payload.title.chars().count() > MAX_NAME_LEN {
        return Err(AppError::BadRequest(format!("Widget title must be 1-{MAX_NAME_LEN} characters")));
    }
    if !VISUALIZATION_TYPES.contains(&payload.visualization_type.as_str()) {
        return Err(AppError::BadRequest(format!(
            "Unsupported visualization_type '{}' (use one of: {})",
            payload.visualization_type,
            VISUALIZATION_TYPES.join(", ")
        )));
    }
    let query_sql = payload.query_sql.as_deref().map(str::trim).filter(|q| !q.is_empty());
    if query_sql.is_some_and(|q| q.len() > MAX_SQL_LEN) {
        return Err(AppError::BadRequest(format!("Widget SQL is longer than {MAX_SQL_LEN} characters")));
    }
    let data_json = match &payload.data {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => {
            let text = v.to_string();
            if text.len() > MAX_DATA_BYTES {
                return Err(AppError::BadRequest(format!("Widget data is larger than {} KB", MAX_DATA_BYTES / 1024)));
            }
            Some(text)
        }
    };
    if query_sql.is_none() && data_json.is_none() && payload.query_id.is_none() {
        return Err(AppError::BadRequest("A widget needs query_sql or data".into()));
    }
    Ok(NewWidget { query_id: payload.query_id, query_sql, data_json, payload })
}

fn validate_widgets(widgets: &[CreateWidgetPayload]) -> Result<Vec<NewWidget<'_>>, AppError> {
    if widgets.len() > MAX_WIDGETS {
        return Err(AppError::BadRequest(format!("A dashboard can hold at most {MAX_WIDGETS} widgets")));
    }
    widgets.iter().map(validate_widget).collect()
}

async fn insert_widget<'e, E>(executor: E, dashboard_id: i64, w: &NewWidget<'_>) -> Result<DashboardWidget, AppError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    Ok(sqlx::query_as::<_, DashboardWidget>(
        "INSERT INTO dashboard_widgets
         (dashboard_id, query_id, query_sql, data_json, title, visualization_type, position_x, position_y, width, height)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING *"
    )
    .bind(dashboard_id)
    .bind(w.query_id)
    .bind(w.query_sql)
    .bind(&w.data_json)
    .bind(w.payload.title.trim())
    .bind(&w.payload.visualization_type)
    .bind(w.payload.position_x.max(0))
    .bind(w.payload.position_y.max(0))
    .bind(w.payload.width.max(1))
    .bind(w.payload.height.max(1))
    .fetch_one(executor)
    .await?)
}

async fn owned_dashboard(pool: &DbPool, user_id: i64, dashboard_id: i64) -> Result<Dashboard, AppError> {
    sqlx::query_as::<_, Dashboard>("SELECT * FROM dashboards WHERE id = ? AND user_id = ?")
        .bind(dashboard_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Dashboard not found".into()))
}

pub async fn create_dashboard(
    pool: &DbPool,
    req: &HttpRequest,
    payload: CreateDashboardPayload,
) -> Result<DashboardWithWidgets, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    create_dashboard_for(pool, user_id, &payload).await
}

pub(crate) async fn create_dashboard_for(
    pool: &DbPool,
    user_id: i64,
    payload: &CreateDashboardPayload,
) -> Result<DashboardWithWidgets, AppError> {
    validate_name(&payload.name)?;
    let widgets = validate_widgets(payload.widgets.as_deref().unwrap_or_default())?;

    let mut tx = pool.begin().await?;
    let dashboard = sqlx::query_as::<_, Dashboard>(
        "INSERT INTO dashboards (user_id, name, description) VALUES (?, ?, ?)
         RETURNING *"
    )
    .bind(user_id)
    .bind(payload.name.trim())
    .bind(&payload.description)
    .fetch_one(&mut *tx)
    .await?;

    let mut saved = Vec::with_capacity(widgets.len());
    for w in &widgets {
        saved.push(insert_widget(&mut *tx, dashboard.id, w).await?);
    }
    tx.commit().await?;

    Ok(DashboardWithWidgets { dashboard, widgets: saved })
}

pub async fn replace_dashboard(
    pool: &DbPool,
    req: &HttpRequest,
    dashboard_id: i64,
    payload: CreateDashboardPayload,
) -> Result<DashboardWithWidgets, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    replace_dashboard_for(pool, user_id, dashboard_id, &payload).await
}

/// Renames the dashboard and, when `widgets` is given, replaces all of its
/// widgets in one transaction.
pub(crate) async fn replace_dashboard_for(
    pool: &DbPool,
    user_id: i64,
    dashboard_id: i64,
    payload: &CreateDashboardPayload,
) -> Result<DashboardWithWidgets, AppError> {
    validate_name(&payload.name)?;
    let widgets = payload.widgets.as_deref().map(validate_widgets).transpose()?;
    owned_dashboard(pool, user_id, dashboard_id).await?;

    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE dashboards SET name = ?, description = ?, updated_at = CURRENT_TIMESTAMP
         WHERE id = ? AND user_id = ?"
    )
    .bind(payload.name.trim())
    .bind(&payload.description)
    .bind(dashboard_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    if let Some(widgets) = &widgets {
        sqlx::query("DELETE FROM dashboard_widgets WHERE dashboard_id = ?")
            .bind(dashboard_id)
            .execute(&mut *tx)
            .await?;
        for w in widgets {
            insert_widget(&mut *tx, dashboard_id, w).await?;
        }
    }
    tx.commit().await?;

    get_dashboard_for(pool, user_id, dashboard_id).await
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
    get_dashboard_for(pool, user_id, dashboard_id).await
}

pub(crate) async fn get_dashboard_for(
    pool: &DbPool,
    user_id: i64,
    dashboard_id: i64,
) -> Result<DashboardWithWidgets, AppError> {
    let dashboard = owned_dashboard(pool, user_id, dashboard_id).await?;
    let widgets = sqlx::query_as::<_, DashboardWidget>(
        "SELECT * FROM dashboard_widgets WHERE dashboard_id = ? ORDER BY id"
    )
    .bind(dashboard_id)
    .fetch_all(pool)
    .await?;

    Ok(DashboardWithWidgets { dashboard, widgets })
}

pub async fn add_widget_to_dashboard(
    pool: &DbPool,
    req: &HttpRequest,
    dashboard_id: i64,
    payload: CreateWidgetPayload,
) -> Result<DashboardWidget, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    owned_dashboard(pool, user_id, dashboard_id).await?;
    let new_widget = validate_widget(&payload)?;
    let widget = insert_widget(pool, dashboard_id, &new_widget).await?;

    // Record preference for chart-type suggestions
    if let Some(query_id) = payload.query_id {
        sqlx::query(
            "INSERT INTO widget_preferences (user_id, query_id, visualization_type, selection_count, last_selected_at)
             VALUES (?, ?, ?, 1, CURRENT_TIMESTAMP)
             ON CONFLICT(user_id, query_id, visualization_type)
             DO UPDATE SET
                selection_count = selection_count + 1,
                last_selected_at = CURRENT_TIMESTAMP"
        )
        .bind(user_id)
        .bind(query_id)
        .bind(&payload.visualization_type)
        .execute(pool)
        .await?;
    }

    sqlx::query("UPDATE dashboards SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(dashboard_id)
        .execute(pool)
        .await?;

    Ok(widget)
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
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Widget not found".into()))?;

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

    delete_dashboard_for(pool, user_id, dashboard_id).await
}

/// SQLite foreign keys are not enforced here, so widgets are removed explicitly.
pub(crate) async fn delete_dashboard_for(pool: &DbPool, user_id: i64, dashboard_id: i64) -> Result<(), AppError> {
    owned_dashboard(pool, user_id, dashboard_id).await?;
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM dashboard_widgets WHERE dashboard_id = ?")
        .bind(dashboard_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM dashboards WHERE id = ? AND user_id = ?")
        .bind(dashboard_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn pool_with_user() -> (DbPool, i64) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        let id: i64 = sqlx::query_scalar("INSERT INTO users (email) VALUES ('a@example.com') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
        (pool, id)
    }

    fn payload(v: serde_json::Value) -> CreateDashboardPayload {
        serde_json::from_value(v).unwrap()
    }

    #[tokio::test]
    async fn saves_query_and_snapshot_widgets_then_replaces_them() {
        let (pool, user) = pool_with_user().await;
        let created = create_dashboard_for(&pool, user, &payload(json!({
            "name": " Blocks ",
            "description": "latest",
            "widgets": [
                {"query_sql": "SELECT block_number, transaction_count FROM blocks LIMIT 5",
                 "title": "Txs per block", "visualization_type": "bar",
                 "position_x": 10, "position_y": 20, "width": 400, "height": 300},
                {"data": [{"label": "Total events", "value": 42}],
                 "title": "Total events", "visualization_type": "number",
                 "position_x": 0, "position_y": 0, "width": 200, "height": 150}
            ]
        }))).await.unwrap();
        assert_eq!(created.dashboard.name, "Blocks");
        assert_eq!(created.widgets.len(), 2);
        assert!(created.widgets[0].query_sql.is_some() && created.widgets[0].data_json.is_none());
        let data: serde_json::Value = serde_json::from_str(created.widgets[1].data_json.as_ref().unwrap()).unwrap();
        assert_eq!(data[0]["value"], 42);

        let id = created.dashboard.id;
        let loaded = get_dashboard_for(&pool, user, id).await.unwrap();
        assert_eq!(loaded.widgets.len(), 2);
        assert_eq!(loaded.widgets[0].position_x, 10);

        let replaced = replace_dashboard_for(&pool, user, id, &payload(json!({
            "name": "Renamed",
            "widgets": [{"query_sql": "SELECT * FROM transactions", "title": "Txs",
                         "visualization_type": "table", "position_x": 0, "position_y": 0,
                         "width": 500, "height": 300}]
        }))).await.unwrap();
        assert_eq!(replaced.dashboard.name, "Renamed");
        assert_eq!(replaced.widgets.len(), 1);
        assert_eq!(replaced.widgets[0].visualization_type, "table");

        // Another user can neither read, overwrite nor delete it.
        assert!(matches!(delete_dashboard_for(&pool, user + 1, id).await, Err(AppError::NotFound(_))));
        assert!(matches!(get_dashboard_for(&pool, user + 1, id).await, Err(AppError::NotFound(_))));
        assert!(matches!(
            replace_dashboard_for(&pool, user + 1, id, &payload(json!({"name": "x"}))).await,
            Err(AppError::NotFound(_))
        ));

        delete_dashboard_for(&pool, user, id).await.unwrap();
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dashboard_widgets").fetch_one(&pool).await.unwrap();
        assert_eq!(left, 0);
    }

    #[tokio::test]
    async fn rejects_invalid_widgets() {
        let (pool, user) = pool_with_user().await;
        let base = |w: serde_json::Value| payload(json!({"name": "D", "widgets": [w]}));
        let no_source = base(json!({"title": "t", "visualization_type": "bar",
            "position_x": 0, "position_y": 0, "width": 1, "height": 1}));
        assert!(matches!(create_dashboard_for(&pool, user, &no_source).await, Err(AppError::BadRequest(_))));
        let bad_type = base(json!({"title": "t", "visualization_type": "gauge", "query_sql": "SELECT 1 FROM blocks",
            "position_x": 0, "position_y": 0, "width": 1, "height": 1}));
        assert!(matches!(create_dashboard_for(&pool, user, &bad_type).await, Err(AppError::BadRequest(_))));
        assert!(matches!(
            create_dashboard_for(&pool, user, &payload(json!({"name": "  "}))).await,
            Err(AppError::BadRequest(_))
        ));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dashboards").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
    }
}
