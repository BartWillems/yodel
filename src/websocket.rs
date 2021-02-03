use std::time::{Duration, Instant};

use actix::prelude::*;
use actix_web::web::Data;
use actix_web::{web, HttpRequest, Responder};

use actix_web_actors::ws;

use crate::jobs;
use crate::jobs::JobServer;

/// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// route used for game updates
pub(crate) async fn route(
    req: HttpRequest,
    stream: web::Payload,
    srv: Data<Addr<JobServer>>,
) -> impl Responder {
    ws::start(
        WebsocketConnection {
            id: 0,
            hb: Instant::now(),
            server: srv.get_ref().clone(),
        },
        &req,
        stream,
    )
}

struct WebsocketConnection {
    /// unique session id
    /// Get's filled in when connecting
    id: usize,
    /// Client must send ping at least once per 10 seconds (CLIENT_TIMEOUT),
    /// otherwise we drop connection.
    hb: Instant,
    server: Addr<JobServer>,
}

impl Actor for WebsocketConnection {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // we'll start heartbeat process on session start.
        self.hb(ctx);
        let addr = ctx.address();
        self.server
            .send(jobs::Connect {
                addr: addr.recipient(),
            })
            .into_actor(self)
            .then(|res, act, ctx| {
                match res {
                    Ok(res) => act.id = res,
                    // something is wrong with notification server
                    Err(e) => {
                        error!("unable to start websocket connection: {}", e);
                        ctx.stop();
                    }
                }
                fut::ready(())
            })
            .wait(ctx);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        self.server.do_send(jobs::Disconnect { id: self.id });
        Running::Stop
    }
}

/// Handle messages from server, we simply send it to peer websocket
impl Handler<jobs::JobAction> for WebsocketConnection {
    type Result = ();

    fn handle(&mut self, notification: jobs::JobAction, ctx: &mut Self::Context) {
        debug!("about to send the client something");
        ctx.text(serde_json::to_string(&notification).unwrap_or_default());
    }
}

/// WebSocket message handler
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WebsocketConnection {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Err(_) => {
                ctx.stop();
                return;
            }
            Ok(msg) => msg,
        };

        trace!("Websocket received message: {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            ws::Message::Pong(_) => {
                self.hb = Instant::now();
            }
            ws::Message::Text(_) => {
                debug!("ignoring incoming messages");
            }
            ws::Message::Binary(_) => debug!("Unexpected binary"),
            ws::Message::Close(reason) => {
                ctx.close(reason);
                ctx.stop();
            }
            ws::Message::Continuation(_) => {
                ctx.stop();
            }
            ws::Message::Nop => (),
        }
    }
}

impl WebsocketConnection {
    /// helper method that sends ping to client every second.
    ///
    /// also this method checks heartbeats from client
    fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                error!("Websocket Client heartbeat failed, disconnecting!");

                act.server.do_send(jobs::Disconnect { id: act.id });

                ctx.stop();

                return;
            }

            ctx.ping(b"");
        });
    }
}
