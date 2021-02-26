use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::process::Command;

use actix::prelude::*;
use actix_web::web::Json;
use actix_web::{get, post, web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use rand::{self, rngs::ThreadRng, Rng};
use serde::{Deserialize, Serialize};

use crate::config::Location;
use crate::errors::YodelError;

#[derive(Clone)]
pub(crate) struct JobServer {
    jobs: HashSet<Job>,
    sessions: HashMap<usize, Recipient<JobResponse>>,
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

    // Send a message to all connected clients
    fn broadcast(&self, msg: &JobResponse) {
        for session in self.sessions.values() {
            let _ = session.do_send(msg.clone());
        }
    }

    pub(crate) fn start_job(&mut self, job: Job, addr: Addr<JobServer>) {
        info!("starting job");
        std::thread::spawn(move || {
            let res = Command::new("youtube-dl")
                .current_dir(job.location.path())
                .arg("--no-overwrite")
                .arg("--all-subs")
                .arg("--embed-subs")
                .arg("-o")
                .arg("%(title)s.mp4")
                .arg(&job.url)
                .output();

            debug!("finished");
            match res {
                Ok(output) => {
                    if output.status.success() {
                        info!("job succeeded!");
                        addr.do_send(JobResponse::Finished(job));
                    } else {
                        let reason = String::from_utf8_lossy(&output.stderr).to_string();
                        error!("youtube-dl failed: {:?}", reason);
                        addr.do_send(JobResponse::Failed { job, reason });
                    }
                }
                Err(reason) => {
                    // this is a server error
                    error!("job startup failed: {}", reason);
                    addr.do_send(JobResponse::Failed {
                        job,
                        reason: reason.to_string(),
                    });
                }
            };
        });
    }

    fn search_title(&mut self, job: Job, addr: Addr<JobServer>) {
        std::thread::spawn(move || {
            let res = Command::new("youtube-dl")
            .arg("--get-title")
            .arg(&job.url)
            .output();

        match res {
            Ok(output) => {
                if output.status.success() {
                    info!("job succeeded!");
                            addr.do_send(VideoTitle {
                        job,
                        title: String::from_utf8_lossy(&output.stdout).to_string(),
                    });
                } else {
                    let err = String::from_utf8_lossy(&output.stderr).to_string();
                    error!("unable to fetch video title: {}", err);
                }
            }
            Err(reason)=> {
                error!("video title lookup failure: {}", reason);
            }
        }

        
        });
    }

    fn pending_jobs(&self) -> Vec<Job> {
        self.jobs
            .iter()
            .filter(|job: &&Job| job.in_progress())
            .cloned()
            .collect()
    }

    fn finished_jobs(&self) -> Vec<Job> {
        self.jobs
            .iter()
            .filter(|job: &&Job| job.is_completed())
            .cloned()
            .collect()
    }

    /// Save an existing job with new values
    /// panics if the job didn't exist yet
    fn save(&mut self, job: Job) {
        self.jobs.replace(job).expect("The job should already exist");
    }
}

#[derive(Eq, PartialEq, Debug, Clone, Serialize)]
pub enum JobStatus {
    Finished,
    Failed(String),
    InProgress,
}

#[derive(Debug, Clone, Serialize, Message)]
#[rtype(result = "()")]
#[serde(rename_all = "camelCase")]
pub struct Job {
    url: String,
    title: Option<String>,
    location: Location,
    started_on: DateTime<Utc>,
    status: JobStatus,
}

impl Job {
    fn in_progress(&self) -> bool {
        self.status == JobStatus::InProgress
    }

    #[allow(dead_code)]
    fn has_failed(&self) -> bool {
        match self.status {
            JobStatus::Failed(_) => true,
            _ => false,
        }
    }

    #[allow(dead_code)]
    fn has_succeeded(&self) -> bool {
        self.status == JobStatus::Finished
    }

    /// return all completed jobs, failed or not
    fn is_completed(&self) -> bool {
        !self.in_progress()
    }

    fn set_finished(&mut self) {
        self.status = JobStatus::Finished;
    }

    fn set_failed(&mut self, reason: String) {
        self.status = JobStatus::Failed(reason);
    }

    fn set_title(&mut self, title: String) {
        self.title = Some(title);
    }
}


impl TryFrom<JobRequest> for Job {
    type Error = YodelError;

    fn try_from(request: JobRequest) -> Result<Job, Self::Error> {
        let location = match Location::lookup(&request.location) {
            Some(location) => location,
            None => {
                return Err(YodelError::BadRequest("Invalid Location".to_string()));
            }
        };

        Ok(Job {
            url: request.url,
            title: None,
            location,
            started_on: Utc::now(),
            status: JobStatus::InProgress,
        })
    }
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

impl fmt::Display for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(title) = &self.title {
            write!(f, "{}", title)
        } else {
            write!(f, "{}", self.url)
        }
    }
}

#[derive(Deserialize, Debug, Message)]
#[rtype(result = "Result<Job, YodelError>")]
struct JobRequest {
    url: String,
    location: String,
}

impl Handler<JobRequest> for JobServer {
    type Result = Result<Job, YodelError>;

    fn handle(&mut self, request: JobRequest, ctx: &mut Context<Self>) -> Self::Result {
        debug!("Request received: {:?}", request);

        let job = Job::try_from(request)?;

        if self.jobs.insert(job.clone()) {
            self.start_job(job.clone(), ctx.address());
            self.search_title(job.clone(), ctx.address());
            self.broadcast(
                JobResponse::PendingJobs(self.pending_jobs()).as_ref()
            );
            Ok(job)
        } else {
            Err(YodelError::Conflict(job.to_string()))
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct VideoTitle {
    job: Job,
    title: String,
}

impl Handler<VideoTitle> for JobServer {
    type Result = ();

    fn handle(&mut self, video_title: VideoTitle, _: &mut Context<Self>) -> Self::Result {
        let mut job = self.jobs.take(&video_title.job).expect("The job can not be none");
        let finished = job.is_completed();
        job.set_title(video_title.title);
        self.jobs.insert(job);

        if finished {
            self.broadcast(JobResponse::CompletedJobs(self.finished_jobs()).as_ref());
        } else {
            self.broadcast(JobResponse::PendingJobs(self.pending_jobs()).as_ref());
        }
    }
}

#[derive(Message)]
#[rtype(usize)]
pub(crate) struct Connect {
    pub(crate) addr: Recipient<JobResponse>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub id: usize,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<Job>, std::io::Error>")]
pub enum JobQuery {
    Pending,
    Completed,
}

/// User facing messages
#[derive(Debug, Message, Serialize, Clone)]
#[rtype(result = "()")]
pub(crate) enum JobResponse {
    // Start(Job),
    Finished(Job),
    /// job finished, but unsuccessfully
    Failed {
        job: Job,
        reason: String,
    },
    PendingJobs(Vec<Job>),
    CompletedJobs(Vec<Job>),
}

impl AsRef<JobResponse> for JobResponse {
    fn as_ref(&self) -> &JobResponse {
        &self
    }
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

impl Handler<JobQuery> for JobServer {
    type Result = Result<Vec<Job>, std::io::Error>;

    fn handle(&mut self, query: JobQuery, _: &mut Context<Self>) -> Self::Result {
        match query {
            JobQuery::Pending => Ok(self.pending_jobs()),
            JobQuery::Completed => Ok(self.finished_jobs()),
        }
    }
}

impl Handler<JobResponse> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: JobResponse, _ctx: &mut Context<Self>) {
        info!("Request received: {:?}", msg);
        match msg.clone() {
            JobResponse::Finished(mut job) => {
                job.set_finished();
                self.save(job);
                self.broadcast(&msg);
                self.broadcast(JobResponse::PendingJobs(self.pending_jobs()).as_ref());
                self.broadcast(JobResponse::CompletedJobs(self.finished_jobs()).as_ref());
            }
            JobResponse::Failed {mut job, reason } => {
                job.set_failed(reason);
                self.save(job);
                self.broadcast(&msg);
                self.broadcast(JobResponse::PendingJobs(self.pending_jobs()).as_ref());
                self.broadcast(JobResponse::CompletedJobs(self.finished_jobs()).as_ref());
            }
            _ => (),
        }
    }
}

#[post("/jobs")]
async fn create_job(
    request: Json<JobRequest>,
    job_server: web::Data<actix::Addr<JobServer>>,
) -> Result<actix_web::HttpResponse, YodelError> {
    let res = job_server.send(request.into_inner()).await?;

    match res {
        Ok(job) => Ok(HttpResponse::Accepted().json(job)),
        Err(e) => Err(e),
    }
}

#[get("/jobs")]
async fn pending_jobs(job_server: web::Data<actix::Addr<JobServer>>) -> impl Responder {
    let jobs: Vec<Job> = job_server
        .send(JobQuery::Pending)
        .await
        .expect("Actix message error")
        .expect("This should never happen");
    HttpResponse::Ok().json(jobs)
}

#[get("/completed-jobs")]
async fn completed_jobs(job_server: web::Data<actix::Addr<JobServer>>) -> impl Responder {
    let jobs: Vec<Job> = job_server
        .send(JobQuery::Completed)
        .await
        .expect("Actix message error")
        .expect("This should never happen");
    HttpResponse::Ok().json(jobs)
}
