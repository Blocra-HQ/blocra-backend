use actix_web::web;
use crate::handlers::payment;
use crate::middleware::auth::auth_middleware;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/payments")
            // Public routes
            .route("/plans", web::get().to(payment::get_plans))
            .route("/chains", web::get().to(payment::get_payment_chains))
            
            // Protected routes
            .route("/status", web::get().to(payment::get_subscription_status).wrap(auth_middleware()))
            .route("/one-time", web::post().to(payment::create_one_time_payment).wrap(auth_middleware()))
            .route("/subscribe", web::post().to(payment::create_subscription).wrap(auth_middleware()))
            .route("/cancel", web::post().to(payment::cancel_subscription).wrap(auth_middleware()))
            .route("/check-usage", web::post().to(payment::check_and_track_report_usage).wrap(auth_middleware()))
            .route("/history", web::get().to(payment::get_payment_history).wrap(auth_middleware()))
            .route("/reports", web::get().to(payment::get_report_history).wrap(auth_middleware()))
    );
}
