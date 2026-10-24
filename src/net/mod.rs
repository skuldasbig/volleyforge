//! Networking — VolleyForge's replacement for the original's DirectPlay 4.
//!
//! # What the original did
//!
//! The original created a DirectPlay 4 peer (`BV_GraphicsUnpacker.c:436`,
//! caption *"Initialisierung von DirectPlay"*) and used it for two very
//! different things:
//!
//! 1. **The lobby.** The host published a session with a random name
//!    (`"game N"`, 12 bytes) and the flags `0x2044`
//!    (`JOINREQUIRED | MIGRATEHOST | KEEPALIVE`), the client enumerated the
//!    running sessions (at most ten of them) and joined one.
//! 2. **The match traffic.** A fixed 40 byte packet
//!    ([`packet::PACKET_LEN`]) carried the commands listed in
//!    [`packet::Cmd`].
//!
//! # Who owns the truth
//!
//! The original is **not** a lockstep game, and the host is the only side
//! that simulates anything:
//!
//! * The host runs the normal match loop (`FUN_00445890` -> `FUN_004443E8`)
//!   and sends the complete state every frame with `Cmd::State`
//!   (`BV_Phys_H_443e8.c:735-747`).
//! * The guest runs a *mirror* loop (`FUN_0044BF98`) that never steps the
//!   physics; it only draws the state it receives
//!   (`BV_Menu_0044BF98.c:134-140`) and forwards its own input to the host
//!   with `Cmd::Input` (`BV_Menu_0044BF98.c:318-336`).
//! * The host applies that input to the guest's blob
//!   (`BV_Phys_H_443e8.c:233-237`) — which is why the three flags of
//!   `Cmd::Input` are exactly the input flags of player 2
//!   (`+0x9782DC` left, `+0x9782DD` right, `+0x9782DE` jump).
//!
//! VolleyForge keeps that model: the host simulates, the guest mirrors.
//!
//! # What DirectPlay did and this module does instead
//!
//! | original                | VolleyForge                                  |
//! |-------------------------|----------------------------------------------|
//! | session enumeration     | [`Cmd::Discover`] / [`Cmd::Announce`] broadcast |
//! | joining a session       | [`Cmd::Join`] / [`Cmd::Welcome`]                |
//! | DirectPlay peer         | one UDP socket                                 |
//! | `FUN_0044BF60` queue    | the OS socket buffer                            |

pub mod packet;
pub mod udp;

#[allow(unused_imports)]
pub use packet::{Cmd, Packet, PACKET_LEN};
#[allow(unused_imports)]
pub use udp::{Event, FoundSession, Point, Role, Session, State, DEFAULT_PORT};

pub use udp::parse_address;
