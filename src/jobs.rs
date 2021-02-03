use std::collections::{HashMap, HashSet};
use std::process::Command;

use actix::prelude::*;
use actix_web::web::Json;
use actix_web::{get, post, web, HttpResponse, Responder};
use rand::{self, rngs::ThreadRng, Rng};
use serde::{Deserialize, Serialize};

use crate::config::Location;

#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Message)]
#[rtype(result = "()")]
pub(crate) struct Job {
    url: String,
    location: Location,
}

impl Job {
    pub fn new(url: String, location: Location) -> Job {
        Job { url, location }
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
        for (_, session) in &self.sessions {
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
                .status();

            info!("finished");
            match res {
                Ok(dink) => {
                    info!("job succeeded! {:?}", dink);
                    addr.do_send(JobAction::Finished(job.clone()));
                }
                Err(e) => {
                    error!("job failed: {}", e);
                    addr.do_send(JobAction::Failed(job.clone()));
                }
            };
        });
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

#[derive(Debug, Message, Serialize, Clone)]
#[rtype(result = "()")]
pub(crate) enum JobAction {
    Start(Job),
    Finished(Job),
    PendingJobs(Vec<Job>),
    Rejected(Job),
    Failed(Job),
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

impl Handler<JobAction> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: JobAction, _ctx: &mut Context<Self>) {
        info!("Request received: {:?}", msg);
        match msg.clone() {
            JobAction::Start(job) => {
                if self.jobs.insert(job.clone()) {
                    self.broadcast(msg);
                    self.start_job_actix(job, _ctx.address());
                } else {
                    self.broadcast(JobAction::Rejected(job));
                }
            }
            JobAction::Finished(job) | JobAction::Failed(job) => {
                self.jobs.remove(&job);
                self.broadcast(msg);
            }
            _ => (),
        }

        let pending_jobs: Vec<Job> = self.jobs.iter().cloned().collect();
        self.broadcast(JobAction::PendingJobs(pending_jobs.clone()));
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

    HttpResponse::Ok().json(job)
}

#[get("/jobs")]
async fn get_jobs() -> impl Responder {
    let jobs: Vec<Job> = Vec::new();
    HttpResponse::Ok().json(jobs)
}
