use actix_web::{error::ResponseError, HttpResponse};
use derive_more::Display;
use std::convert::From;

#[derive(Debug, Display)]
pub enum YodelError {
    InternalServerError,
    #[display(fmt = "BadRequest: {}", _0)]
    BadRequest(String),
    #[display(fmt = "Job already exists: {}", _0)]
    Conflict(String),
    TooManyJobs,
}

impl ResponseError for YodelError {
    fn error_response(&self) -> HttpResponse {
        match self {
            YodelError::InternalServerError => {
                HttpResponse::InternalServerError().json("Internal Server Error, Please try later")
            }
            YodelError::BadRequest(ref message) => HttpResponse::BadRequest().json(message),
            YodelError::Conflict(ref message) => HttpResponse::Conflict().json(message),
            YodelError::TooManyJobs => {
                HttpResponse::TooManyRequests().json("Too many running jobs")
            }
        }
    }
}

impl From<std::io::Error> for YodelError {
    fn from(error: std::io::Error) -> YodelError {
        error!("I/O Error: {:?}", error);
        YodelError::InternalServerError
    }
}

impl<E> From<actix_threadpool::BlockingError<E>> for YodelError
where
    E: std::fmt::Debug,
    E: Into<YodelError>,
{
    fn from(error: actix_threadpool::BlockingError<E>) -> YodelError {
        match error {
            actix_threadpool::BlockingError::Error(e) => e.into(),
            actix_threadpool::BlockingError::Canceled => {
                error!("actix thread canceled");
                YodelError::InternalServerError
            }
        }
    }
}

impl From<actix::MailboxError> for YodelError {
    fn from(error: actix::MailboxError) -> YodelError {
        error!("actix mailbox error: {}", error);
        YodelError::InternalServerError
    }
}
