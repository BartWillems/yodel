use std::collections::HashMap;
use std::path::PathBuf;

use actix_web::{get, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

lazy_static::lazy_static! {
    static ref LOCATIONS: HashMap<String, PathBuf> = {
        let contents = std::fs::read_to_string("config.yaml").unwrap();
        let items: HashMap<String, PathBuf> = serde_yaml::from_str(&contents).unwrap();

        items
    };
}

#[get("/config")]
async fn get_config() -> impl Responder {
    HttpResponse::Ok().json(LOCATIONS.clone())
}

#[derive(Hash, Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Location {
    name: String,
    path: std::path::PathBuf,
}

impl<'a> Location {
    pub(crate) fn lookup(name: &str) -> Option<Location> {
        let path: PathBuf = LOCATIONS.get(name)?.clone();

        Some(Location {
            name: name.into(),
            path,
        })
    }

    pub(crate) fn path(&'a self) -> &'a PathBuf {
        &self.path
    }
}
