//! The UDP transport: one socket, two roles, the original's message flow.
//!
//! See the module documentation of [`crate::net`] for the mapping between the
//! original's DirectPlay traffic and this module.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use crate::log::{debug, info, warn};

use super::packet::{Cmd, Packet, NAME_BYTES};

/// The well-known port the session list is searched on.
///
/// The original let DirectPlay pick the ports (`2300`..`2303` by default);
/// VolleyForge uses one fixed port so that a broadcast can find the host.
pub const DEFAULT_PORT: u16 = 24680;

/// How long a peer may stay silent before the session counts as lost.
///
/// The original had no such check - DirectPlay closed the session on its own.
pub const PEER_TIMEOUT: Duration = Duration::from_secs(10);

/// The magic word in a discovery packet: `"VF"` plus a version.
const DISCOVER_MAGIC: u32 = 0x5646_0001;

/// Which side of the match this session is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// The side that simulates the match.
    Host,
    /// The side that mirrors the host's state.
    Guest,
}

/// A position in the original's sub-pixel units.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// The complete game state the host sends every frame.
///
/// The layout is the original's (`BV_Phys_H_443e8.c:735-747`):
///
/// ```text
/// word 0  ball x      word 5  player 2 y
/// word 1  ball y      word 6  time >> 3
/// word 2  player 1 x  word 7  player 1 animation
/// word 3  player 1 y  word 8  player 2 animation
/// word 4  player 2 x
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct State {
    pub ball_x: i32,
    pub ball_y: i32,
    pub p0: Point,
    pub p1: Point,
    pub anim0: u32,
    pub anim1: u32,
}

/// A session a host announced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundSession {
    /// The name the host chose.
    pub name: String,
    /// Where the host is.
    pub addr: SocketAddr,
    /// The port the host plays on.
    pub port: u16,
}

/// Everything that can arrive over the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A host answered our discovery broadcast.
    Found(FoundSession),
    /// A guest wants to join (host side only).
    Join {
        addr: SocketAddr,
        name: String,
    },
    /// The host accepted us (guest side only).
    Welcome {
        slot: usize,
        host_colour: u8,
        host_name: String,
    },
    /// The host sent the game state.
    State(State),
    /// The host sent the score.
    Score {
        p0: u8,
        p1: u8,
        /// Which player serves; `0xFF` means nobody (first rally).
        server: u8,
        /// The receiver of the score is one point from winning.
        match_point: bool,
        /// The match is over.
        over: bool,
    },
    /// The guest sent its input.
    Input {
        jump: bool,
        left: bool,
        right: bool,
    },
    /// The host wants a sound played.
    Sound(u8),
    /// A player announced a team colour.
    Colour { player: usize, colour: u8 },
    /// The peer stopped answering.
    Lost,
}

/// The socket plus the peer we are talking to.
struct Udp {
    socket: UdpSocket,
    peer: Option<SocketAddr>,
}

impl Udp {
    fn bind(port: u16) -> io::Result<Udp> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))?;
        socket.set_nonblocking(true)?;
        Ok(Udp { socket, peer: None })
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    fn set_peer(&mut self, addr: SocketAddr) {
        self.peer = Some(addr);
    }

    fn send_to(&self, packet: &Packet, addr: SocketAddr) -> io::Result<()> {
        self.socket.send_to(&packet.encode(), addr).map(|_| ())
    }

    fn send(&self, packet: &Packet) -> io::Result<()> {
        let peer = self
            .peer
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "no peer yet"))?;
        self.send_to(packet, peer)
    }

    fn broadcast(&self, packet: &Packet, port: u16) -> io::Result<()> {
        self.socket.set_broadcast(true)?;
        self.socket
            .send_to(&packet.encode(), (Ipv4Addr::BROADCAST, port))
            .map(|_| ())
    }

    fn recv(&self) -> Option<(Packet, SocketAddr)> {
        let mut buf = [0u8; 128];
        match self.socket.recv_from(&mut buf) {
            Ok((n, addr)) => Packet::decode(&buf[..n]).map(|p| (p, addr)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(e) => {
                debug!(error = %e, "udp receive failed");
                None
            }
        }
    }
}

/// A network session: either the host of a match or a guest in one.
pub struct Session {
    role: Role,
    udp: Udp,
    /// The port discovery broadcasts to.
    discovery_port: u16,
    /// Our own session name.
    name: String,
    /// Our own team colour.
    colour: u8,
    /// The sessions a guest has found so far.
    pub found: Vec<FoundSession>,
    /// True once the two sides are talking to each other.
    pub connected: bool,
    /// The player slot we play: the host is 0, the guest is 1.
    pub slot: usize,
    /// While the host waits for a guest it answers discovery requests.
    announcing: bool,
    last_rx: Option<Instant>,
    /// The round trip time measured with the timestamp of the state packet.
    ping: Option<Duration>,
}

impl Session {
    /// Start a host: bind the port and wait for a guest.
    pub fn host(name: impl Into<String>, port: u16) -> io::Result<Session> {
        let udp = Udp::bind(port)?;
        Ok(Session {
            role: Role::Host,
            udp,
            discovery_port: port,
            name: name.into(),
            colour: 0,
            found: Vec::new(),
            connected: false,
            slot: 0,
            announcing: true,
            last_rx: None,
            ping: None,
        })
    }

    /// Start a guest: bind an arbitrary port and listen for the host.
    pub fn guest(discovery_port: u16) -> io::Result<Session> {
        let udp = Udp::bind(0)?;
        Ok(Session {
            role: Role::Guest,
            udp,
            discovery_port,
            name: String::new(),
            colour: 0,
            found: Vec::new(),
            connected: false,
            slot: 1,
            announcing: false,
            last_rx: None,
            ping: None,
        })
    }

    /// Which side we are.
    pub fn role(&self) -> Role {
        self.role
    }

    /// The address our socket is bound to.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.udp.local_addr()
    }

    /// The name we publish.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set the name we publish.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Set our own team colour.
    pub fn set_colour(&mut self, colour: u8) {
        self.colour = colour;
    }

    /// The measured round trip time, if a state packet arrived.
    pub fn ping(&self) -> Option<Duration> {
        self.ping
    }

    /// Ask the network for the running sessions.
    ///
    /// This is the replacement for DirectPlay's session enumeration: every
    /// host that is still waiting for a guest answers with a broadcast.
    pub fn discover(&mut self) -> io::Result<()> {
        let mut packet = Packet::new(Cmd::Discover);
        packet.set_word(0, DISCOVER_MAGIC);
        self.udp.broadcast(&packet, self.discovery_port)
    }

    /// Ask a found session to take us.
    pub fn join(&mut self, session: &FoundSession) -> io::Result<()> {
        self.join_addr(session.addr)
    }

    /// Ask a host at a known address to take us.
    ///
    /// The original's client screen asks for the host's IP (`HOST - IP:`)
    /// and connects to it directly, without asking the lobby first.
    pub fn join_addr(&mut self, addr: SocketAddr) -> io::Result<()> {
        let mut packet = Packet::new(Cmd::Join);
        packet.set_name(0, NAME_BYTES, &self.name);
        self.udp.set_peer(addr);
        self.udp.send_to(&packet, addr)
    }

    /// Leave the session.
    pub fn say_goodbye(&mut self) {
        if self.connected {
            let _ = self.udp.send(&Packet::new(Cmd::Bye));
        }
        self.connected = false;
    }

    /// Read everything that arrived and turn it into events.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        while let Some((packet, from)) = self.udp.recv() {
            self.last_rx = Some(Instant::now());
            let Some(cmd) = packet.cmd() else {
                debug!(byte = packet.cmd_byte(), "unknown command ignored");
                continue;
            };
            match cmd {
                Cmd::Discover => {
                    if self.role == Role::Host && self.announcing {
                        let mut answer = Packet::new(Cmd::Announce);
                        answer.set_name(0, NAME_BYTES, &self.name);
                        answer.set_word(6, self.discovery_port as u32);
                        if let Err(e) = self.udp.send_to(&answer, from) {
                            debug!(error = %e, "cannot answer a discovery");
                        }
                    }
                }
                Cmd::Announce => {
                    if self.role == Role::Guest {
                        let name = packet.name(0, NAME_BYTES);
                        let port = packet.word(6) as u16;
                        let found = FoundSession {
                            name: name.clone(),
                            addr: from,
                            port,
                        };
                        if !self.found.iter().any(|s| s.addr == found.addr) {
                            self.found.push(found.clone());
                            info!(host = %name, %from, "session found");
                            events.push(Event::Found(found));
                        }
                    }
                }
                Cmd::Join => {
                    if self.role == Role::Host && !self.connected {
                        let name = packet.name(0, NAME_BYTES);
                        self.udp.set_peer(from);
                        self.connected = true;
                        self.announcing = false;
                        let mut welcome = Packet::new(Cmd::Welcome);
                        welcome.set_word(0, self.slot as u32 + 1);
                        welcome.set_word(1, self.colour as u32);
                        welcome.set_name(3, 12, &self.name);
                        let _ = self.udp.send(&welcome);
                        info!(guest = %name, %from, "guest joined");
                        events.push(Event::Join {
                            addr: from,
                            name,
                        });
                    }
                }
                Cmd::Welcome => {
                    if self.role == Role::Guest && !self.connected {
                        self.connected = true;
                        self.slot = packet.word(0) as usize;
                        let host_colour = packet.word(1) as u8;
                        let host_name = packet.name(3, 12);
                        info!(host = %host_name, "welcome received");
                        events.push(Event::Welcome {
                            slot: self.slot,
                            host_colour,
                            host_name,
                        });
                    }
                }
                Cmd::State => {
                    let state = State {
                        ball_x: packet.word(0) as i32,
                        ball_y: packet.word(1) as i32,
                        p0: Point {
                            x: packet.word(2) as i32,
                            y: packet.word(3) as i32,
                        },
                        p1: Point {
                            x: packet.word(4) as i32,
                            y: packet.word(5) as i32,
                        },
                        anim0: packet.word(7),
                        anim1: packet.word(8),
                    };
                    // The original put the tick count into the packet
                    // (`GetTickCount() >> 3`); we use it to measure the
                    // round trip time.
                    let stamp = packet.word(6);
                    if stamp != 0 {
                        let now = now_ticks() >> 3;
                        self.ping = Some(Duration::from_millis(
                            now.saturating_sub(stamp) as u64,
                        ));
                    }
                    events.push(Event::State(state));
                }
                Cmd::Score => {
                    events.push(Event::Score {
                        p0: packet.byte(0),
                        p1: packet.byte(1),
                        server: packet.byte(2),
                        match_point: packet.byte(3) != 0,
                        over: packet.byte(4) != 0,
                    });
                }
                Cmd::Input => {
                    events.push(Event::Input {
                        jump: packet.word(0) != 0,
                        left: packet.word(1) != 0,
                        right: packet.word(2) != 0,
                    });
                }
                Cmd::Sound => {
                    events.push(Event::Sound(packet.word(0) as u8));
                }
                Cmd::Colour => {
                    events.push(Event::Colour {
                        player: packet.word(0) as usize,
                        colour: packet.word(1) as u8,
                    });
                }
                Cmd::Bye => {
                    if self.connected {
                        self.connected = false;
                        events.push(Event::Lost);
                    }
                }
            }
        }
        self.check_timeout(&mut events);
        events
    }

    /// Send the complete game state — the host's job.
    pub fn send_state(&mut self, state: &State) -> io::Result<()> {
        let mut packet = Packet::new(Cmd::State);
        packet.set_word(0, state.ball_x as u32);
        packet.set_word(1, state.ball_y as u32);
        packet.set_word(2, state.p0.x as u32);
        packet.set_word(3, state.p0.y as u32);
        packet.set_word(4, state.p1.x as u32);
        packet.set_word(5, state.p1.y as u32);
        packet.set_word(6, now_ticks() >> 3);
        packet.set_word(7, state.anim0);
        packet.set_word(8, state.anim1);
        self.udp.send(&packet)
    }

    /// Send our input — the guest's job.
    pub fn send_input(&mut self, jump: bool, left: bool, right: bool) -> io::Result<()> {
        let mut packet = Packet::new(Cmd::Input);
        packet.set_word(0, jump as u32);
        packet.set_word(1, left as u32);
        packet.set_word(2, right as u32);
        self.udp.send(&packet)
    }

    /// Send the score — the host's job.
    pub fn send_score(
        &mut self,
        p0: u8,
        p1: u8,
        server: u8,
        match_point: bool,
        over: bool,
    ) -> io::Result<()> {
        let mut packet = Packet::new(Cmd::Score);
        packet.set_byte(0, p0);
        packet.set_byte(1, p1);
        packet.set_byte(2, server);
        packet.set_byte(3, match_point as u8);
        packet.set_byte(4, over as u8);
        self.udp.send(&packet)
    }

    /// Tell the guest to play a sound.
    pub fn send_sound(&mut self, index: u8) -> io::Result<()> {
        let mut packet = Packet::new(Cmd::Sound);
        packet.set_word(0, index as u32);
        self.udp.send(&packet)
    }

    /// Announce our team colour.
    pub fn send_colour(&mut self, player: usize, colour: u8) -> io::Result<()> {
        let mut packet = Packet::new(Cmd::Colour);
        packet.set_word(0, player as u32);
        packet.set_word(1, colour as u32);
        self.udp.send(&packet)
    }

    /// The peer has been silent for too long.
    fn check_timeout(&mut self, events: &mut Vec<Event>) {
        if !self.connected {
            return;
        }
        if let Some(last) = self.last_rx {
            if last.elapsed() > PEER_TIMEOUT {
                warn!("peer timed out");
                self.connected = false;
                events.push(Event::Lost);
            }
        }
    }
}

/// Parse an address the player typed.
///
/// The client screen of the original asks for the host's IP (`HOST - IP:`),
/// so the port takes a plain address as well as one with a port.
pub fn parse_address(text: &str) -> io::Result<SocketAddr> {
    let text = text.trim();
    if text.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "no address"));
    }
    if let Ok(addr) = text.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let ip: std::net::IpAddr = text.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("'{text}' is not an address"),
        )
    })?;
    Ok(SocketAddr::new(ip, DEFAULT_PORT))
}

/// A millisecond counter, the counterpart of the original's `GetTickCount`.
pub fn now_ticks() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // The original's tick count wrapped around after 49 days; so does ours.
    (millis & 0xFFFF_FFFF) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host and a guest on the loopback interface.
    ///
    /// Every test uses its own port: the tests run in parallel and two UDP
    /// sockets cannot share a port.
    fn pair(offset: u16) -> (Session, Session) {
        let port = DEFAULT_PORT + offset;
        let host = Session::host("game 1", port).expect("host binds");
        let guest = Session::guest(port).expect("guest binds");
        (host, guest)
    }

    #[test]
    fn a_guest_finds_the_host_and_joins() {
        let (mut host, mut guest) = pair(20);
        guest.set_name("guest");
        guest.discover().expect("broadcast works");

        // The host answers the broadcast.
        let events = host.poll();
        assert!(events.is_empty(), "the host does not report a join yet");

        // The guest receives the answer.
        let events = guest.poll();
        assert_eq!(events.len(), 1, "one session found: {events:?}");
        let Event::Found(found) = &events[0] else {
            panic!("expected a found session, got {events:?}");
        };
        assert_eq!(found.name, "game 1");
        assert_eq!(found.port, DEFAULT_PORT + 20);

        // And joins it.
        guest.join(found).expect("join is sent");
        let events = host.poll();
        assert_eq!(events.len(), 1, "the host sees the join: {events:?}");
        let Event::Join { name, .. } = &events[0] else {
            panic!("expected a join, got {events:?}");
        };
        assert_eq!(name, "guest");

        // The host welcomes the guest.
        let events = guest.poll();
        assert_eq!(events.len(), 1, "the guest is welcomed: {events:?}");
        let Event::Welcome { slot, .. } = &events[0] else {
            panic!("expected a welcome, got {events:?}");
        };
        assert_eq!(*slot, 1, "the guest plays as player 2");
    }

    #[test]
    fn the_state_survives_the_round_trip() {
        let (mut host, mut guest) = pair(21);
        guest.set_name("guest");
        guest.discover().expect("broadcast works");
        host.poll();
        let Event::Found(found) = guest.poll().remove(0) else {
            panic!("the host was not found");
        };
        guest.join(&found).expect("join is sent");
        host.poll();
        guest.poll();

        let state = State {
            ball_x: 1234,
            ball_y: -4321,
            p0: Point { x: 200, y: 100 },
            p1: Point { x: 600, y: 100 },
            anim0: 7,
            anim1: 9,
        };
        host.send_state(&state).expect("state is sent");
        let events = guest.poll();
        let Event::State(received) = &events[0] else {
            panic!("expected a state, got {events:?}");
        };
        assert_eq!(*received, state);
    }

    #[test]
    fn the_input_survives_the_round_trip() {
        let (mut host, mut guest) = pair(22);
        guest.set_name("guest");
        guest.discover().expect("broadcast works");
        host.poll();
        let Event::Found(found) = guest.poll().remove(0) else {
            panic!("the host was not found");
        };
        guest.join(&found).expect("join is sent");
        host.poll();
        guest.poll();

        guest.send_input(true, false, true).expect("input is sent");
        let events = host.poll();
        let Event::Input { jump, left, right } = &events[0] else {
            panic!("expected an input, got {events:?}");
        };
        assert!(*jump);
        assert!(!*left);
        assert!(*right);
    }

    #[test]
    fn the_score_survives_the_round_trip() {
        let (mut host, mut guest) = pair(23);
        guest.set_name("guest");
        guest.discover().expect("broadcast works");
        host.poll();
        let Event::Found(found) = guest.poll().remove(0) else {
            panic!("the host was not found");
        };
        guest.join(&found).expect("join is sent");
        host.poll();
        guest.poll();

        host.send_score(11, 7, 1, true, false).expect("score is sent");
        let events = guest.poll();
        let Event::Score {
            p0,
            p1,
            server,
            match_point,
            over,
        } = &events[0]
        else {
            panic!("expected a score, got {events:?}");
        };
        assert_eq!(*p0, 11);
        assert_eq!(*p1, 7);
        assert_eq!(*server, 1);
        assert!(*match_point);
        assert!(!*over);
    }

    #[test]
    fn a_guest_that_does_not_join_stays_alone() {
        let (mut host, mut guest) = pair(24);
        guest.discover().expect("broadcast works");
        host.poll();
        let events = guest.poll();
        assert_eq!(events.len(), 1);
        assert!(!guest.connected);
        assert!(!host.connected);
    }

    /// An address without a port gets the default port.
    #[test]
    fn an_address_without_a_port_gets_the_default_one() {
        let addr = parse_address("192.168.1.5").expect("parses");
        assert_eq!(addr.port(), DEFAULT_PORT);
        let addr = parse_address("192.168.1.5:1234").expect("parses");
        assert_eq!(addr.port(), 1234);
        assert!(
            parse_address("not an address").is_err(),
            "garbage must not be accepted"
        );
    }

    /// The guest can join a host whose address it knows - that is the
    /// `HOST - IP:` field of the original's client screen.
    #[test]
    fn the_guest_joins_a_known_address() {
        let (mut host, mut guest) = pair(26);
        guest.set_name("guest");
        let port = host.local_addr().expect("the host has an address").port();
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().expect("an address");
        guest.join_addr(addr).expect("the join request is sent");

        let events = host.poll();
        let Event::Join { name, .. } = &events[0] else {
            panic!("expected a join, got {events:?}");
        };
        assert_eq!(name, "guest");

        // And the host takes the guest.
        let events = guest.poll();
        assert!(
            matches!(events[0], Event::Welcome { slot: 1, .. }),
            "expected a welcome, got {events:?}"
        );
    }

    /// The whole chain of the original's asymmetric model: the guest's input
    /// moves the blob **on the host**, and the guest sees the result.
    #[test]
    fn the_guests_input_moves_the_hosts_blob() {
        use crate::player::{Control, Intent};
        use crate::rules::Match;

        let (mut host, mut guest) = pair(25);
        guest.set_name("guest");
        guest.discover().expect("broadcast works");
        host.poll();
        let Event::Found(found) = guest.poll().remove(0) else {
            panic!("the host was not found");
        };
        guest.join(&found).expect("join is sent");
        host.poll();
        guest.poll();

        // The host simulates, the guest mirrors - and the blob of the guest
        // is steered over the wire (the original's control mode 50).
        let mut host_match = Match::new(0x5EED_1234, None);
        host_match.players[1].control = Control::Network;
        let mut guest_match = Match::new(0x5EED_1234, None);
        guest_match.players[0].control = Control::Network;

        // The guest walks to the right: jump, left, right.
        guest.send_input(false, false, true).expect("input is sent");
        let events = host.poll();
        let Event::Input { left, right, .. } = &events[0] else {
            panic!("expected an input, got {events:?}");
        };
        assert!(!left);
        assert!(right);
        host_match.players[1].intent = Intent {
            left: *left,
            right: *right,
            jump: false,
        };

        // The host moves the blob and sends the state.
        let before = host_match.players[1].x;
        host_match.players[1].step(1);
        assert_ne!(
            host_match.players[1].x, before,
            "the blob of the guest must move on the host"
        );
        let state = host_match.net_state();
        host.send_state(&state).expect("state is sent");

        // The guest sees the new position.
        let events = guest.poll();
        let Event::State(received) = &events[0] else {
            panic!("expected a state, got {events:?}");
        };
        guest_match.apply_net_state(received);
        assert_eq!(guest_match.players[1].x, host_match.players[1].x);
        assert_eq!(guest_match.ball.x, host_match.ball.x);
    }
}
