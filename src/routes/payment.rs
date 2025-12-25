use actix_web::web;
use crate::handlers::payment;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/payments")
            // Public routes
            .route("/plans", web::get().to(payment::get_plans))
            .route("/chains", web::get().to(payment::get_payment_chains))
            
            // Protected routes (auth handled in handlers via HttpRequest)
            .route("/status", web::get().to(payment::get_subscription_status))
            .route("/one-time", web::post().to(payment::create_one_time_payment))
            .route("/subscribe", web::post().to(payment::create_subscription))
            .route("/cancel", web::post().to(payment::cancel_subscription))
            .route("/check-usage", web::post().to(payment::check_and_track_report_usage))
            .route("/history", web::get().to(payment::get_payment_history))
            .route("/reports", web::get().to(payment::get_report_history))
    );
}
