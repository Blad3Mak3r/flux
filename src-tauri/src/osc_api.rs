//! OSC show operation over UDP and length-prefixed TCP. Network work never reaches an audio callback.
use crate::{
    commands::SharedSession,
    show_commands,
    transport_commands::{self, SharedOutput},
};
use flux_engine::{
    control::{OscMessage, decode_osc, osc_action},
    control_in::Input,
    osc::encode,
    transport::Command,
};
use flux_project::OscArg;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

const LIMIT: usize = 65_507;
const MAX_CLIENTS: usize = 32;
const MAX_PENDING: usize = 262_144;

pub struct OscServer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl OscServer {
    pub fn bind(
        app: AppHandle,
        address: &str,
        port: u16,
        tcp: bool,
        legacy: Sender<Input>,
    ) -> io::Result<Self> {
        let address: IpAddr = address
            .parse()
            .map_err(|_| io::Error::other("invalid OSC listen address"))?;
        let udp = UdpSocket::bind((address, port))?;
        udp.set_nonblocking(true)?;
        let port = udp.local_addr()?.port();
        let tcp = if tcp {
            let listener = TcpListener::bind((address, port))?;
            listener.set_nonblocking(true)?;
            Some(listener)
        } else {
            None
        };
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let worker = thread::Builder::new().name("flux-osc-api".into()).spawn(move || {
            let mut server = Server {app, legacy, cache: RequestCache::default(), subscriptions: HashMap::new(), revision: 0, last_state: Value::Null};
            let mut clients = Vec::new();
            let mut packet = [0u8; LIMIT];
            while !stopped.load(Ordering::Acquire) {
                for _ in 0..64 {
                    match udp.recv_from(&mut packet) {
                        Ok((size, peer)) => { for reply in server.packet(peer, &packet[..size], false) { let _ = udp.send_to(&reply, peer); } }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }
                if let Some(listener) = &tcp {
                    while clients.len() < MAX_CLIENTS {
                        match listener.accept() {
                            Ok((stream, peer)) => { if stream.set_nonblocking(true).is_ok() { clients.push(Client { stream, peer, input: Vec::new(), output: Vec::new(), sent: 0 }); } }
                            Err(_) => break,
                        }
                    }
                }
                clients.retain_mut(|client| {let alive = client.step(&mut server); if !alive {server.subscriptions.remove(&client.peer);} alive});
                let state = server.snapshot();
                if state != server.last_state {server.revision += 1; server.last_state = state.clone();}
                let now = Instant::now();
                let revision = server.revision;
                server.subscriptions.retain(|peer, subscription| {
                    if !subscription.tcp && now >= subscription.expires { return false; }
                    let status_changed = state.get("turn") != subscription.last_status.get("turn")
                        || state.get("go") != subscription.last_status.get("go")
                        || state.get("tracks") != subscription.last_status.get("tracks")
                        || state.get("outputs") != subscription.last_status.get("outputs")
                        || state.get("status") != subscription.last_status.get("status") || state.get("entryId") != subscription.last_status.get("entryId");
                    if now >= subscription.due || status_changed {
                        subscription.sequence += 1;
                        let packet = reply("/flux/v1/state", &subscription.id, true, &json!({"sequence":subscription.sequence,"revision":revision,"state":state}));
                        if let Some(client) = clients.iter_mut().find(|c| c.peer == *peer) { client.queue(&packet); }
                        else { let _ = udp.send_to(&packet, subscription.target); }
                        subscription.due = now + Duration::from_micros(1_000_000 / u64::from(subscription.hz));
                        subscription.last_status = state.clone();
                    }
                    true
                });
                thread::sleep(Duration::from_millis(5));
            }
        })?;
        Ok(Self {
            port,
            stop,
            thread: Some(worker),
        })
    }
    pub fn port(&self) -> u16 {
        self.port
    }
}
impl Drop for OscServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

struct Client {
    stream: TcpStream,
    peer: SocketAddr,
    input: Vec<u8>,
    output: Vec<u8>,
    sent: usize,
}
impl Client {
    fn queue(&mut self, packet: &[u8]) {
        self.output
            .extend_from_slice(&(packet.len() as u32).to_be_bytes());
        self.output.extend_from_slice(packet);
    }
    fn step(&mut self, server: &mut Server) -> bool {
        let mut buffer = [0u8; 8192];
        for _ in 0..8 {
            match self.stream.read(&mut buffer) {
                Ok(0) => {
                    server.subscriptions.remove(&self.peer);
                    return false;
                }
                Ok(n) => self.input.extend_from_slice(&buffer[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    server.subscriptions.remove(&self.peer);
                    return false;
                }
            }
        }
        if self.input.len() > MAX_PENDING {
            return false;
        }
        for _ in 0..32 {
            let packet = match take_frame(&mut self.input) {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(()) => return false,
            };
            for reply in server.packet(self.peer, &packet, true) {
                self.queue(&reply);
            }
        }
        if self.output.len() > MAX_PENDING {
            server.subscriptions.remove(&self.peer);
            return false;
        }
        if self.sent < self.output.len() {
            match self.stream.write(&self.output[self.sent..]) {
                Ok(0) => return false,
                Ok(n) => self.sent += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => return false,
            }
        }
        if self.sent == self.output.len() {
            self.output.clear();
            self.sent = 0;
        }
        true
    }
}

fn take_frame(input: &mut Vec<u8>) -> Result<Option<Vec<u8>>, ()> {
    let Some(prefix) = input.get(..4) else {
        return Ok(None);
    };
    let size = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
    if size == 0 || size > LIMIT {
        return Err(());
    }
    if input.len() < size + 4 {
        return Ok(None);
    }
    let packet = input[4..size + 4].to_vec();
    input.drain(..size + 4);
    Ok(Some(packet))
}

struct Subscription {
    tcp: bool,
    sequence: u64,
    id: String,
    target: SocketAddr,
    hz: u32,
    due: Instant,
    expires: Instant,
    last_status: Value,
}
struct Cached {
    peer: SocketAddr,
    id: String,
    message: OscMessage,
    response: Vec<u8>,
    expires: Instant,
}
#[derive(Default)]
struct RequestCache(VecDeque<Cached>);
impl RequestCache {
    fn lookup(
        &mut self,
        peer: SocketAddr,
        id: &str,
        message: &OscMessage,
        now: Instant,
    ) -> Option<Vec<u8>> {
        self.0.retain(|c| c.expires > now);
        let cached = self.0.iter().find(|c| c.peer == peer && c.id == id)?;
        Some(if cached.message == *message {
            cached.response.clone()
        } else {
            reply(
                "/flux/v1/reply",
                id,
                false,
                &json!({"error":"requestIdConflict"}),
            )
        })
    }
    fn remember(&mut self, cached: Cached) {
        if self.0.len() >= 256 {
            self.0.pop_front();
        }
        self.0.push_back(cached);
    }
}

struct Server {
    app: AppHandle,
    legacy: Sender<Input>,
    cache: RequestCache,
    subscriptions: HashMap<SocketAddr, Subscription>,
    revision: u64,
    last_state: Value,
}
impl Server {
    fn packet(&mut self, peer: SocketAddr, packet: &[u8], tcp: bool) -> Vec<Vec<u8>> {
        let messages = decode_osc(packet);
        messages
            .into_iter()
            .filter_map(|message| {
                if !message.address.starts_with("/flux/v1/") {
                    if let Some(action) = osc_action(&message) {
                        let _ = self.legacy.send(Input::Action(action));
                    }
                    return None;
                }
                let id = match message.args.first() {
                    Some(OscArg::String(id)) if !id.is_empty() && id.len() <= 128 => id.clone(),
                    _ => {
                        return Some(reply(
                            "/flux/v1/reply",
                            "",
                            false,
                            &json!({"error":"requestIdRequired"}),
                        ));
                    }
                };
                let now = Instant::now();
                if let Some(response) = self.cache.lookup(peer, &id, &message, now) {
                    return Some(response);
                }
                let result = self.execute(peer, &id, &message, tcp);
                let response = match result {
                    Ok(data) => reply("/flux/v1/reply", &id, true, &data),
                    Err(error) => reply("/flux/v1/reply", &id, false, &json!({"error":error})),
                };
                self.cache.remember(Cached {
                    peer,
                    id,
                    message,
                    response: response.clone(),
                    expires: now + Duration::from_secs(60),
                });
                Some(response)
            })
            .collect()
    }

    fn snapshot(&self) -> Value {
        let output = self.app.state::<SharedOutput>();
        if let Some(state) = show_commands::state(&output) {
            json!({"setlistId":state.setlist_id,"entryId":state.entry_id,"timelineId":state.timeline_id,"name":state.name,"next":state.next,
                "status":state.tick.status,"go":state.tick.go,"turn":state.tick.turn,"devampPending":state.tick.devamp_pending,"position":state.position,"frameRate":state.frame_rate,"currentCue":state.live.current,"nextCue":state.live.next,"running":state.live.running,"outputs":transport_commands::show_reader(&output).map(|r|r.audio_routes()),"tracks":self.track_snapshot()})
        } else {
            json!({"position":transport_commands::show_reader(&output).map(|r|r.position())})
        }
    }

    fn execute(
        &mut self,
        peer: SocketAddr,
        id: &str,
        message: &OscMessage,
        tcp: bool,
    ) -> Result<Value, String> {
        let args = &message.args[1..];
        let text = |index| match args.get(index) {
            Some(OscArg::String(s)) => Ok(s.clone()),
            _ => Err("stringArgumentRequired".to_string()),
        };
        let output = self.app.state::<SharedOutput>();
        match message.address.as_str() {
            "/flux/v1/state/get" => {
                return Ok(json!({"revision":self.revision,"state":self.snapshot()}));
            }
            "/flux/v1/setlists/get" => return Ok(json!(self.project()?.setlists)),
            "/flux/v1/outputs/get" => {
                return Ok(
                    json!({"info":transport_commands::running_info(&output),"audio":transport_commands::show_reader(&output).map(|r|r.audio_routes()),"position":transport_commands::show_reader(&output).map(|r|r.position())}),
                );
            }
            "/flux/v1/tracks/get" => return Ok(self.track_snapshot()),
            "/flux/v1/timelines/get" => {
                return Ok(json!(
                    self.project()?
                        .timelines
                        .iter()
                        .map(|t| json!({"id":t.id,"name":t.name,"timecode":t.timecode}))
                        .collect::<Vec<_>>()
                ));
            }
            "/flux/v1/timeline/select" => {
                tauri::async_runtime::block_on(show_commands::prepare_timeline(
                    self.app.clone(),
                    &text(0)?,
                ))?;
            }
            "/flux/v1/cues/get" => {
                let (_, cues) = transport_commands::snapshot(&output).ok_or("outputNotRunning")?;
                return Ok(json!(cues.cues()));
            }
            "/flux/v1/cue/goto" => {
                let (_, cues) = transport_commands::snapshot(&output).ok_or("outputNotRunning")?;
                let cue_id = text(0)?;
                let cue = cues
                    .cues()
                    .iter()
                    .find(|c| c.id == cue_id)
                    .ok_or("unknownCue")?;
                self.transport(Command::Seek(cue.frame))?;
            }
            "/flux/v1/setlist/select" => {
                tauri::async_runtime::block_on(show_commands::prepare(self.app.clone(), text(0)?))?;
            }
            "/flux/v1/entry/prepare" => show_commands::execute(
                &self.app,
                &output,
                show_commands::Action::Prepare { entry_id: text(0)? },
            )
            .map_err(|e| e.to_string())?,
            "/flux/v1/go" => show_commands::execute(&self.app, &output, show_commands::Action::Go)
                .map_err(|e| e.to_string())?,
            "/flux/v1/devamp" => {
                show_commands::execute(&self.app, &output, show_commands::Action::Devamp)
                    .map_err(|e| e.to_string())?
            }
            "/flux/v1/devamp/cancel" => {
                show_commands::execute(&self.app, &output, show_commands::Action::CancelDevamp)
                    .map_err(|e| e.to_string())?
            }
            "/flux/v1/transport/play" => self.transport(Command::Play)?,
            "/flux/v1/transport/pause" => self.transport(Command::Pause)?,
            "/flux/v1/transport/stop" => self.transport(Command::Stop)?,
            "/flux/v1/transport/seek" => {
                self.transport(Command::Seek(integer(args.first())? as u64))?
            }
            "/flux/v1/track/gain" | "/flux/v1/track/mute" => {
                let track_id = text(0)?;
                let value = number(args.get(1))?;
                crate::show_commands::track_level(
                    &self.app,
                    &track_id,
                    value,
                    message.address.ends_with("mute"),
                )?;
            }
            "/flux/v1/subscribe" => {
                let hz = match args.first() {
                    None => 10,
                    arg => integer(arg)?,
                };
                if !(1..=30).contains(&hz) {
                    return Err("rateMustBe1To30".into());
                }
                let port = match args.get(1) {
                    None => peer.port(),
                    arg => u16::try_from(integer(arg)?)
                        .ok()
                        .filter(|p| *p > 0)
                        .ok_or("invalidReplyPort")?,
                };
                if self.subscriptions.len() >= MAX_CLIENTS
                    && !self.subscriptions.contains_key(&peer)
                {
                    return Err("tooManySubscribers".into());
                }
                self.subscriptions.insert(
                    peer,
                    Subscription {
                        tcp,
                        sequence: 0,
                        id: id.into(),
                        target: SocketAddr::new(peer.ip(), port),
                        hz: hz as u32,
                        due: Instant::now(),
                        expires: Instant::now() + Duration::from_secs(30),
                        last_status: Value::Null,
                    },
                );
            }
            "/flux/v1/unsubscribe" => {
                self.subscriptions.remove(&peer);
            }
            _ => return Err("unknownAddress".into()),
        }
        Ok(json!({"accepted":true}))
    }

    fn transport(&self, command: Command) -> Result<(), String> {
        crate::voice_commands::send_transport(
            &self.app.state::<Arc<crate::voice_commands::VoiceState>>(),
            &self.app.state::<SharedOutput>(),
            command,
        )
        .map_err(|e| e.to_string())
    }
    fn track_snapshot(&self) -> Value {
        let output = self.app.state::<SharedOutput>();
        if let Some((plan, tick)) = transport_commands::show_reader(&output).and_then(|r| r.show())
        {
            let mix = plan
                .mix
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            json!(
                plan.entries[tick.entry]
                    .timeline
                    .tracks
                    .iter()
                    .map(|t| {
                        let (gain, muted) = mix
                            .get(&t.id)
                            .copied()
                            .unwrap_or((t.gain_db.unwrap_or(0.0), t.muted));
                        json!({"id":t.id,"name":t.name,"gainDb":gain,"muted":muted})
                    })
                    .collect::<Vec<_>>()
            )
        } else {
            self.project().ok().and_then(|p| {
                let active = self.app.state::<SharedSession>().lock().unwrap_or_else(std::sync::PoisonError::into_inner).active();
                p.timeline(&active).map(|t|json!(t.tracks.iter().map(|t|json!({"id":t.id,"name":t.name,"gainDb":t.gain_db,"muted":t.muted})).collect::<Vec<_>>()))
            }).unwrap_or(Value::Null)
        }
    }

    fn project(&self) -> Result<flux_project::Project, String> {
        self.app
            .state::<SharedSession>()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .document()
            .map(|d| d.project)
            .ok_or_else(|| "noProject".into())
    }
}

fn number(arg: Option<&OscArg>) -> Result<f32, String> {
    match arg {
        Some(OscArg::Int(v)) => Ok(*v as f32),
        Some(OscArg::Float(v)) if v.is_finite() => Ok(*v),
        Some(OscArg::Bool(v)) => Ok(if *v { 1.0 } else { 0.0 }),
        _ => Err("numericArgumentRequired".into()),
    }
}
fn integer(arg: Option<&OscArg>) -> Result<i32, String> {
    match arg {
        Some(OscArg::Int(v)) if *v >= 0 => Ok(*v),
        _ => Err("nonnegativeIntegerRequired".into()),
    }
}
fn reply(address: &str, id: &str, ok: bool, value: &Value) -> Vec<u8> {
    let packet = encode(
        address,
        &[
            OscArg::String(id.into()),
            OscArg::Bool(ok),
            OscArg::String(value.to_string()),
        ],
    );
    if packet.len() > LIMIT {
        encode(
            "/flux/v1/reply",
            &[
                OscArg::String(id.into()),
                OscArg::Bool(false),
                OscArg::String("{\"error\":\"responseTooLarge\"}".into()),
            ],
        )
    } else {
        packet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_is_cached_but_new_commands_and_other_clients_are_independent() {
        let mut cache = RequestCache::default();
        let peer: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let message = OscMessage {
            address: "/flux/v1/go".into(),
            args: vec![OscArg::String("go-1".into())],
        };
        let now = Instant::now();
        cache.remember(Cached {
            peer,
            id: "go-1".into(),
            message: message.clone(),
            response: vec![1, 2, 3],
            expires: now + Duration::from_secs(60),
        });
        assert_eq!(
            cache.lookup(peer, "go-1", &message, now),
            Some(vec![1, 2, 3])
        );
        assert!(cache.lookup(peer, "go-2", &message, now).is_none());
        assert!(
            cache
                .lookup("127.0.0.1:9001".parse().unwrap(), "go-1", &message, now)
                .is_none()
        );
        let changed = OscMessage {
            address: "/flux/v1/devamp".into(),
            ..message.clone()
        };
        let error = cache.lookup(peer, "go-1", &changed, now).unwrap();
        let response = decode_osc(&error);
        assert!(matches!(&response[0].args[1], OscArg::Bool(false)));
        assert!(
            cache
                .lookup(peer, "go-1", &message, now + Duration::from_secs(61))
                .is_none()
        );
    }

    #[test]
    fn tcp_framing_retains_partial_and_consecutive_packets() {
        let mut input = vec![0, 0, 0];
        assert_eq!(take_frame(&mut input), Ok(None));
        input.extend_from_slice(&[4, 1, 2]);
        assert_eq!(take_frame(&mut input), Ok(None));
        input.extend_from_slice(&[3, 4, 0, 0, 0, 1, 9]);
        assert_eq!(take_frame(&mut input), Ok(Some(vec![1, 2, 3, 4])));
        assert_eq!(take_frame(&mut input), Ok(Some(vec![9])));
        assert!(input.is_empty());
    }
    #[test]
    fn tcp_rejects_zero_and_oversized_frames() {
        assert!(take_frame(&mut vec![0; 4]).is_err());
        assert!(take_frame(&mut vec![255; 4]).is_err());
    }
}
