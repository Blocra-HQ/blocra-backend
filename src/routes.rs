use actix_web::web;

mod auth;
mod bounty;
mod admin;
mod contract;
mod dashboard;
mod query;
mod health;
mod feedback;
mod dashboard_builder;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .configure(health::configure)
            .configure(auth::configure)
            .configure(admin::configure)
            .configure(bounty::configure)
            .configure(contract::configure)
            .configure(dashboard::configure)
            .configure(query::configure)
            .configure(feedback::configure)
            .configure(dashboard_builder::configure)
    );
}
