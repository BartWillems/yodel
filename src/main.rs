#[macro_use]
extern crate log;

use actix::prelude::*;
use actix_web::middleware::Logger;
use actix_web::{web, App, HttpServer};

mod config;
mod jobs;
mod websocket;

#[actix_web::main]
async fn main() -> Result<(), terminator::Terminator> {
    init().await?;

    Ok(())
}

async fn init() -> std::io::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let job_server = jobs::JobServer::new().start();
    HttpServer::new(move || {
        App::new()
            .data(job_server.clone())
            .wrap(Logger::default())
            .service(config::get_config)
            .service(jobs::get_jobs)
            .service(jobs::create_job)
            .service(web::resource("/ws").to(websocket::route))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
