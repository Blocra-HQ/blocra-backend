use actix_web::web;

mod auth;
mod admin;
mod contract;
mod dashboard;
mod query;
mod health;
mod feedback;
mod dashboard_builder;
mod contract_transactions;
mod ai;
mod sql;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .configure(health::configure)
            .configure(auth::configure)
            .configure(admin::configure)
            .configure(contract::configure)
            .configure(dashboard::configure)
            .configure(query::configure)
            .configure(feedback::configure)
            .configure(dashboard_builder::configure)
            .configure(contract_transactions::configure)
            .configure(ai::configure)
            .configure(sql::configure)
    );
}
