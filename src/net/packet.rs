//! The wire format — the original's 40 byte DirectPlay payload.
//!
//! The original never defined a structure for it: it kept a 40 byte block in
//! the form's memory at `+0x97CE0C`, wrote the command into the first byte
//! and the payload into the nine 32 bit words behind it. The receive path
//! copied exactly 40 bytes out of the DirectPlay buffer
//! (`FUN_0044BED8`: `FUN_004027e8(dp + 0x48C, buf + 0x97CE0C, 0x28)`) and the
//! send path handed the same block to DirectPlay (`FUN_0044BF60`).
//!
//! ```text
//! offset  size  content
//! 0x00    1     command
//! 0x01    3     padding (always zero)
//! 0x04    4     word 0   -> 0x97CE10
//! 0x08    4     word 1   -> 0x97CE14
//! 0x0C    4     word 2   -> 0x97CE18
//! 0x10    4     word 3   -> 0x97CE1C
//! 0x14    4     word 4   -> 0x97CE20
//! 0x18    4     word 5   -> 0x97CE24
//! 0x1C    4     word 6   -> 0x97CE28
//! 0x20    4     word 7   -> 0x97CE2C
//! 0x24    4     word 8   -> 0x97CE30
//! ```
//!
//! The score packet is the odd one out: the original read it **byte by byte**
//! from `0x97CE10` to `0x97CE14`, so the values live in the low bytes of the
//! first words. [`Packet::byte`] and [`Packet::set_byte`] reproduce that.

/// The length of one packet: `0x28` = 40 bytes.
pub const PACKET_LEN: usize = 0x28;

/// The number of 32 bit words behind the command byte.
pub const WORD_COUNT: usize = 9;

/// How many bytes of a name fit into the word area.
pub const NAME_BYTES: usize = 24;

/// The command byte of a packet.
///
/// Commands 2..=6 are the original's own (`BV_Menu_0044BF98.c:123-148` for the
/// receive side, `BV_Phys_H_443e8.c:735-747` for the state).
///
/// Commands 7..=11 are VolleyForge additions: the original let DirectPlay
/// enumerate the sessions and negotiate the session, so those steps have no
/// counterpart in the game's own packet format. They carry the same 40 byte
/// envelope so that a single socket handles everything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Cmd {
    /// Points: the two scores, who served and whether the match is over.
    ///
    /// Sent by the host (`BV_Phys_H_443e8.c:106`, `:604`,
    /// `BV_Menu_00445890.c:61`).
    Score = 2,
    /// The full game state.
    ///
    /// Sent by the host every frame (`BV_Phys_H_443e8.c:735-747`); the guest
    /// overwrites its own state with it (`BV_Menu_0044BF98.c:134-140`).
    State = 3,
    /// The guest's input: jump, left, right.
    ///
    /// Sent by the guest every frame (`BV_Menu_0044BF98.c:318-336`), read by
    /// the host (`BV_Phys_H_443e8.c:233-237`).
    Input = 4,
    /// A sound to play.
    ///
    /// `BV_MessageByIndex.c:8-14`, played by the guest
    /// (`BV_Menu_0044BF98.c:144-146`).
    Sound = 5,
    /// A player's team colour: player index and colour index.
    ///
    /// `BV_Menu_0044B8B4.c:124` (host) and `BV_Menu_0044BC94.c:65` (guest),
    /// applied by `FUN_0044A784`.
    Colour = 6,
    /// VolleyForge: "is anybody hosting?" — the replacement for DirectPlay's
    /// session enumeration.
    Discover = 7,
    /// VolleyForge: the host's answer to a discovery, carrying the session
    /// name and the UDP port.
    Announce = 8,
    /// VolleyForge: the guest asks to join.
    Join = 9,
    /// VolleyForge: the host accepts the guest and tells it which slot and
    /// which colours it got.
    Welcome = 10,
    /// VolleyForge: a polite goodbye.
    Bye = 11,
}

impl Cmd {
    /// Map a raw command byte back to a command.
    pub fn from_u8(value: u8) -> Option<Cmd> {
        Some(match value {
            2 => Cmd::Score,
            3 => Cmd::State,
            4 => Cmd::Input,
            5 => Cmd::Sound,
            6 => Cmd::Colour,
            7 => Cmd::Discover,
            8 => Cmd::Announce,
            9 => Cmd::Join,
            10 => Cmd::Welcome,
            11 => Cmd::Bye,
            _ => return None,
        })
    }
}

/// One network packet: a command byte plus nine words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Packet {
    cmd: u8,
    words: [u32; WORD_COUNT],
}

impl Packet {
    /// A packet with the given command and a zeroed payload.
    pub fn new(cmd: Cmd) -> Packet {
        Packet {
            cmd: cmd as u8,
            words: [0; WORD_COUNT],
        }
    }

    /// The command of this packet, `None` if the byte is not one we know.
    pub fn cmd(&self) -> Option<Cmd> {
        Cmd::from_u8(self.cmd)
    }

    /// The raw command byte.
    pub fn cmd_byte(&self) -> u8 {
        self.cmd
    }

    /// Read one of the nine words.
    pub fn word(&self, index: usize) -> u32 {
        self.words[index]
    }

    /// Write one of the nine words.
    pub fn set_word(&mut self, index: usize, value: u32) {
        self.words[index] = value;
    }

    /// Read a byte out of the word area.
    ///
    /// The original read the score packet byte by byte from `0x97CE10`, which
    /// is the low byte of word 0; byte 1 is the second byte of word 0 and so
    /// on.
    pub fn byte(&self, index: usize) -> u8 {
        (self.words[index / 4] >> (8 * (index % 4))) as u8
    }

    /// Write a byte into the word area.
    pub fn set_byte(&mut self, index: usize, value: u8) {
        let shift = 8 * (index % 4);
        let mask = 0xFFu32 << shift;
        self.words[index / 4] = (self.words[index / 4] & !mask) | ((value as u32) << shift);
    }

    /// Store a name in the word area, starting at `first_word`.
    ///
    /// The name is UTF-8 and truncated to `max_bytes`; the area is cleared
    /// first so that no leftover of a longer name survives.
    pub fn set_name(&mut self, first_word: usize, max_bytes: usize, name: &str) {
        let words = max_bytes.div_ceil(4);
        for w in 0..words {
            self.words[first_word + w] = 0;
        }
        for (i, byte) in name.as_bytes().iter().copied().enumerate().take(max_bytes) {
            let shift = 8 * (i % 4);
            self.words[first_word + i / 4] |= (byte as u32) << shift;
        }
    }

    /// Read a name back out of the word area.
    pub fn name(&self, first_word: usize, max_bytes: usize) -> String {
        let mut raw = Vec::with_capacity(max_bytes);
        for i in 0..max_bytes {
            raw.push(self.byte(first_word * 4 + i));
        }
        let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..end]).into_owned()
    }

    /// Serialise into the 40 byte wire format.
    pub fn encode(&self) -> [u8; PACKET_LEN] {
        let mut out = [0u8; PACKET_LEN];
        out[0] = self.cmd;
        for (i, word) in self.words.iter().enumerate() {
            let at = 4 + i * 4;
            out[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    /// Parse a packet from the wire.
    ///
    /// Returns `None` for a short or malformed buffer, which is what a random
    /// UDP datagram or a port scan looks like.
    pub fn decode(bytes: &[u8]) -> Option<Packet> {
        if bytes.len() < PACKET_LEN {
            return None;
        }
        let mut words = [0u32; WORD_COUNT];
        for (i, word) in words.iter_mut().enumerate() {
            let at = 4 + i * 4;
            *word = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        }
        Some(Packet {
            cmd: bytes[0],
            words,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_is_forty_bytes() {
        assert_eq!(PACKET_LEN, 0x28);
        assert_eq!(Packet::new(Cmd::State).encode().len(), 40);
    }

    #[test]
    fn round_trip_keeps_the_command_and_the_words() {
        let mut packet = Packet::new(Cmd::State);
        packet.set_word(0, 0x1234_5678);
        packet.set_word(8, 0xDEAD_BEEF);
        let bytes = packet.encode();
        let back = Packet::decode(&bytes).expect("decodes");
        assert_eq!(back.cmd(), Some(Cmd::State));
        assert_eq!(back.word(0), 0x1234_5678);
        assert_eq!(back.word(8), 0xDEAD_BEEF);
        assert_eq!(back, packet);
    }

    #[test]
    fn the_command_sits_in_the_first_byte_and_the_padding_is_zero() {
        let bytes = Packet::new(Cmd::Input).encode();
        assert_eq!(bytes[0], 4);
        assert_eq!(&bytes[1..4], &[0, 0, 0]);
    }

    #[test]
    fn a_short_buffer_is_rejected() {
        assert!(Packet::decode(&[0u8; 39]).is_none());
        assert!(Packet::decode(&[0u8; 40]).is_some());
    }

    #[test]
    fn bytes_live_in_the_low_bytes_of_the_words() {
        // The original read the score packet byte by byte from 0x97CE10.
        let mut packet = Packet::new(Cmd::Score);
        packet.set_byte(0, 7);
        packet.set_byte(1, 3);
        packet.set_byte(4, 1);
        assert_eq!(packet.word(0) & 0xFFFF, 0x0307);
        assert_eq!(packet.byte(0), 7);
        assert_eq!(packet.byte(1), 3);
        assert_eq!(packet.byte(4), 1);
        // Byte 4 is the low byte of word 1 (0x97CE14).
        assert_eq!(packet.word(1) & 0xFF, 1);
        // Byte 3 would be the high byte of word 0 (0x97CE13).
        packet.set_byte(3, 0x80);
        assert_eq!(packet.word(0) >> 24, 0x80);
    }

    #[test]
    fn a_name_survives_the_round_trip() {
        let mut packet = Packet::new(Cmd::Announce);
        packet.set_name(0, NAME_BYTES, "game 3");
        assert_eq!(packet.name(0, NAME_BYTES), "game 3");
    }

    #[test]
    fn a_longer_name_is_truncated_and_the_rest_is_cleared() {
        let mut packet = Packet::new(Cmd::Announce);
        packet.set_name(0, NAME_BYTES, "a very long session name");
        let stored = packet.name(0, NAME_BYTES);
        assert_eq!(stored.len(), NAME_BYTES);
        // Writing a shorter name must not leave the tail of the old one.
        packet.set_name(0, NAME_BYTES, "xy");
        assert_eq!(packet.name(0, NAME_BYTES), "xy");
    }

    #[test]
    fn every_command_round_trips() {
        for cmd in [
            Cmd::Score,
            Cmd::State,
            Cmd::Input,
            Cmd::Sound,
            Cmd::Colour,
            Cmd::Discover,
            Cmd::Announce,
            Cmd::Join,
            Cmd::Welcome,
            Cmd::Bye,
        ] {
            let packet = Packet::new(cmd);
            assert_eq!(Packet::decode(&packet.encode()).unwrap().cmd(), Some(cmd));
        }
    }
}
