#[macro_use]
extern crate log;

use actix::prelude::*;
use actix_cors::Cors;
use actix_files::Files;
use actix_web::middleware::Logger;
use actix_web::{web, App, HttpServer};

mod config;
mod errors;
mod jobs;
mod websocket;

#[actix_web::main]
async fn main() -> Result<(), terminator::Terminator> {
    init().await?;

    Ok(())
}

async fn init() -> std::io::Result<()> {
    if let Err(e) = init_logger() {
        eprintln!("Something went wrong while setting up the logger: {}", e);
        std::process::exit(1);
    }
    let job_server = jobs::JobServer::new().start();
    HttpServer::new(move || {
        App::new()
            .data(job_server.clone())
            .wrap(Logger::default())
            .wrap(Cors::permissive().supports_credentials())
            .service(
                web::scope("/api")
                    .service(config::locations)
                    .service(jobs::pending_jobs)
                    .service(jobs::completed_jobs)
                    .service(jobs::create_job),
            )
            .service(web::resource("/ws").to(websocket::route))
            .service(mount_frontend())
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

fn init_logger() -> std::io::Result<()> {
    let colors = fern::colors::ColoredLevelConfig::default();
    fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                colors.color(record.level()),
                message
            ))
        })
        // Add blanket level filter -
        .level(log::LevelFilter::Debug)
        // - and per-module overrides
        .level_for("hyper", log::LevelFilter::Info)
        // Output to stdout, files, and other Dispatch configurations
        .chain(std::io::stdout())
        .chain(fern::log_file("output.log")?)
        // Apply globally
        .apply()
        // This only fails if a logger was already setup, this is a developer error
        .expect("Unable to setup logger");
    Ok(())
}

#[cfg(target_os = "freebsd")]
fn mount_frontend() -> Files {
    Files::new("/", "frontend").index_file("index.html")
}

#[cfg(not(target_os = "freebsd"))]
fn mount_frontend() -> Files {
    Files::new("/", "frontend/build").index_file("index.html")
}
