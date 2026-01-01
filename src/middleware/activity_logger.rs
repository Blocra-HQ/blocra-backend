use actix_web::{dev::{ServiceRequest, ServiceResponse}, Error, HttpMessage};
use actix_web::dev::{Service, Transform};
use futures::future::{ok, Ready};
use std::task::{Context, Poll};
use std::pin::Pin;
use futures::Future;
use crate::db::DbPool;

pub struct ActivityLogger;

impl<S, B> Transform<S, ServiceRequest> for ActivityLogger
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = ActivityLoggerMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(ActivityLoggerMiddleware { service })
    }
}

pub struct ActivityLoggerMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for ActivityLoggerMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let path = req.path().to_string();
        let method = req.method().to_string();
        let ip = req.peer_addr().map(|addr| addr.ip().to_string());
        let user_agent = req.headers()
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(String::from);

        // Extract user_id from extensions if available
        let user_id = req.extensions().get::<i64>().copied();

        let fut = self.service.call(req);

        Box::pin(async move {
            let res = fut.await?;
            
            // Log activity if user is authenticated and it's an API call
            if let Some(uid) = user_id {
                if path.starts_with("/api/") && method != "GET" {
                    // Get pool from app data
                    if let Some(pool) = res.request().app_data::<actix_web::web::Data<DbPool>>() {
                        let activity_type = match path.as_str() {
                            p if p.contains("/query") => "query",
                            p if p.contains("/contract") => "contract_view",
                            p if p.contains("/dashboard") => "report",
                            _ => "api_call",
                        };

                        let description = format!("{} {}", method, path);
                        
                        // Log asynchronously (don't block response)
                        let pool_clone = pool.clone();
                        let ip_clone = ip.clone();
                        let ua_clone = user_agent.clone();
                        
                        tokio::spawn(async move {
                            let _ = sqlx::query(
                                "INSERT INTO activity_logs (user_id, activity_type, description, ip_address, user_agent) 
                                 VALUES (?, ?, ?, ?, ?)"
                            )
                            .bind(uid)
                            .bind(activity_type)
                            .bind(description)
                            .bind(ip_clone)
                            .bind(ua_clone)
                            .execute(pool_clone.as_ref())
                            .await;
                        });
                    }
                }
            }

            Ok(res)
        })
    }
}
