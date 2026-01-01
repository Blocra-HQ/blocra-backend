use actix_web::{web, HttpResponse, HttpRequest};
use crate::{handlers::dashboard_builder, db::DbPool, models::dashboard::*};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/dashboard-builder")
            .route("", web::post().to(create_dashboard))
            .route("", web::get().to(get_dashboards))
            .route("/{id}", web::get().to(get_dashboard))
            .route("/{id}", web::delete().to(delete_dashboard))
            .route("/{id}/widgets", web::post().to(add_widget))
            .route("/widgets/{id}", web::put().to(update_widget))
            .route("/widgets/{id}", web::delete().to(delete_widget))
            .route("/queries", web::get().to(get_saved_queries))
            .route("/suggest/{query_id}", web::get().to(suggest_chart))
    );
}

async fn create_dashboard(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    payload: web::Json<CreateDashboardPayload>,
) -> Result<HttpResponse, actix_web::Error> {
    let dashboard = dashboard_builder::create_dashboard(&pool, &req, payload.into_inner()).await?;
    Ok(HttpResponse::Created().json(serde_json::json!({
        "success": true,
        "dashboard": dashboard
    })))
}

async fn get_dashboards(
    pool: web::Data<DbPool>,
    req: HttpRequest,
) -> Result<HttpResponse, actix_web::Error> {
    let dashboards = dashboard_builder::get_user_dashboards(&pool, &req).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "dashboards": dashboards
    })))
}

async fn get_dashboard(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> Result<HttpResponse, actix_web::Error> {
    let dashboard_id = path.into_inner();
    let dashboard = dashboard_builder::get_dashboard_with_widgets(&pool, &req, dashboard_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "data": dashboard
    })))
}

async fn add_widget(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    path: web::Path<i64>,
    payload: web::Json<CreateWidgetPayload>,
) -> Result<HttpResponse, actix_web::Error> {
    let dashboard_id = path.into_inner();
    let widget = dashboard_builder::add_widget_to_dashboard(&pool, &req, dashboard_id, payload.into_inner()).await?;
    Ok(HttpResponse::Created().json(serde_json::json!({
        "success": true,
        "widget": widget
    })))
}

async fn update_widget(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    path: web::Path<i64>,
    payload: web::Json<serde_json::Value>,
) -> Result<HttpResponse, actix_web::Error> {
    let widget_id = path.into_inner();
    let position_x = payload["position_x"].as_i64().unwrap_or(0);
    let position_y = payload["position_y"].as_i64().unwrap_or(0);
    let width = payload["width"].as_i64().unwrap_or(400);
    let height = payload["height"].as_i64().unwrap_or(300);

    let widget = dashboard_builder::update_widget_position(&pool, &req, widget_id, position_x, position_y, width, height).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "widget": widget
    })))
}

async fn delete_widget(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> Result<HttpResponse, actix_web::Error> {
    let widget_id = path.into_inner();
    dashboard_builder::delete_widget(&pool, &req, widget_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": "Widget deleted"
    })))
}

async fn delete_dashboard(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> Result<HttpResponse, actix_web::Error> {
    let dashboard_id = path.into_inner();
    dashboard_builder::delete_dashboard(&pool, &req, dashboard_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": "Dashboard deleted"
    })))
}

async fn get_saved_queries(
    pool: web::Data<DbPool>,
    req: HttpRequest,
) -> Result<HttpResponse, actix_web::Error> {
    let queries = dashboard_builder::get_saved_queries(&pool, &req).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "queries": queries
    })))
}

async fn suggest_chart(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> Result<HttpResponse, actix_web::Error> {
    let query_id = path.into_inner();
    let suggestions = dashboard_builder::suggest_chart_type(&pool, &req, query_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "suggestions": suggestions,
        "has_suggestions": !suggestions.is_empty()
    })))
}
