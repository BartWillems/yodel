use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::process::Command;

use actix::prelude::*;
use actix_web::web::Json;
use actix_web::{get, post, web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use rand::{self, rngs::ThreadRng, Rng};
use serde::{Deserialize, Serialize};

use crate::config::Location;

#[derive(Debug, Clone, Serialize, Message)]
#[rtype(result = "()")]
#[serde(rename_all = "camelCase")]
pub struct Job {
    url: String,
    location: Location,
    started_on: DateTime<Utc>,
}

impl Hash for Job {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.url.hash(state);
        self.location.hash(state);
    }
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.url.eq(&other.url) && self.location.eq(&other.location)
    }
}

impl Eq for Job {}

impl Job {
    pub fn new(url: String, location: Location) -> Job {
        Job {
            url,
            location,
            started_on: Utc::now(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct JobServer {
    jobs: HashSet<Job>,
    sessions: HashMap<usize, Recipient<JobAction>>,
    rng: ThreadRng,
}

impl JobServer {
    pub fn new() -> JobServer {
        JobServer {
            jobs: HashSet::new(),
            sessions: HashMap::new(),
            rng: rand::thread_rng(),
        }
    }

    fn broadcast(&self, msg: JobAction) {
        for session in self.sessions.values() {
            let _ = session.do_send(msg.clone());
        }
    }

    pub(crate) fn start_job_actix(&mut self, job: Job, addr: Addr<JobServer>) {
        info!("starting job");
        std::thread::spawn(move || {
            let res = Command::new("youtube-dl")
                .current_dir(job.location.path())
                .arg("--no-overwrite")
                .arg("-o")
                .arg("%(title)s.mp4")
                .arg(&job.url)
                .output();

            debug!("finished");
            match res {
                Ok(output) => {
                    if output.status.success() {
                        info!("job succeeded!");
                        addr.do_send(JobAction::Finished(job.clone()));
                    } else {
                        let error = String::from_utf8_lossy(&output.stderr).to_string();
                        error!("youtube-dl failed: {:?}", error);
                        addr.do_send(JobAction::Failed {
                            job: job.clone(),
                            reason: error,
                        });
                    }
                }
                Err(err) => {
                    // this is a server error
                    error!("job startup failed: {}", err);
                    addr.do_send(JobAction::Failed {
                        job: job.clone(),
                        reason: err.to_string(),
                    });
                }
            };
        });
    }

    fn pending_jobs(&self) -> Vec<Job> {
        self.jobs.iter().cloned().collect()
    }
}

#[derive(Message)]
#[rtype(usize)]
pub(crate) struct Connect {
    pub(crate) addr: Recipient<JobAction>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub id: usize,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<Job>, std::io::Error>")]
pub struct PendingJobs;

#[derive(Debug, Message, Serialize, Clone)]
#[rtype(result = "()")]
pub(crate) enum JobAction {
    Start(Job),
    Finished(Job),
    /// job was already in queue
    Rejected(Job),
    /// job finished, but unsuccessfully
    Failed {
        job: Job,
        reason: String,
    },
    PendingJobs(Vec<Job>),
}

impl Actor for JobServer {
    type Context = Context<Self>;
}

impl Handler<Connect> for JobServer {
    type Result = usize;

    fn handle(&mut self, msg: Connect, _ctx: &mut Context<Self>) -> Self::Result {
        let session_id = self.rng.gen::<usize>();
        self.sessions.insert(session_id, msg.addr);

        info!("new connection!");

        session_id
    }
}

impl Handler<Disconnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
        info!("connection lost");
        self.sessions.remove(&msg.id);
    }
}

impl Handler<PendingJobs> for JobServer {
    type Result = Result<Vec<Job>, std::io::Error>;

    fn handle(&mut self, _: PendingJobs, _: &mut Context<Self>) -> Self::Result {
        Ok(self.pending_jobs())
    }
}

impl Handler<JobAction> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: JobAction, _ctx: &mut Context<Self>) {
        info!("Request received: {:?}", msg);
        match msg.clone() {
            JobAction::Start(job) => {
                if self.jobs.insert(job.clone()) {
                    self.broadcast(msg);
                    self.start_job_actix(job, _ctx.address());
                    self.broadcast(JobAction::PendingJobs(self.pending_jobs()));
                } else {
                    self.broadcast(JobAction::Rejected(job));
                }
            }
            JobAction::Finished(job) | JobAction::Failed { job, reason: _ } => {
                self.jobs.remove(&job);
                self.broadcast(msg);
                self.broadcast(JobAction::PendingJobs(self.pending_jobs()));
            }
            _ => (),
        }
    }
}

#[derive(Deserialize)]
struct JobRequest {
    url: String,
    location: String,
}

#[post("/jobs")]
async fn create_job(
    request: Json<JobRequest>,
    job_server: web::Data<actix::Addr<JobServer>>,
) -> impl Responder {
    let location = match Location::lookup(&request.location) {
        Some(location) => location,
        None => {
            return HttpResponse::NotFound()
                .json(format!("target location {} not found", &request.location))
        }
    };

    let job = Job::new(request.url.clone(), location);

    info!("sending job request to job server");

    job_server.send(JobAction::Start(job.clone())).await.ok();

    HttpResponse::Accepted().json(job)
}

#[get("/jobs")]
async fn get_jobs(job_server: web::Data<actix::Addr<JobServer>>) -> impl Responder {
    let jobs: Vec<Job> = job_server
        .send(PendingJobs)
        .await
        .expect("Actix message error")
        .expect("This should never happen");
    HttpResponse::Ok().json(jobs)
}

#[get("/completed-jobs")]
async fn get_completed_jobs() -> impl Responder {
    let jobs: Vec<Job> = Vec::new();
    HttpResponse::Ok().json(jobs)
}
