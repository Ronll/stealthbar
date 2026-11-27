//! Framing of the dock's vendor HID channel: Airoha RACE packets carried in HID reports 6 (out) and 7 (in),
//! with Turtle Beach's JSON key/value documents inside RACE command 0x9902.

pub const REPORT_OUT: u8 = 0x06;
pub const REPORT_IN: u8 = 0x07;
/// Both reports are 62 bytes including the report id.
pub const REPORT_LEN: usize = 62;

pub const CMD_KVP: u16 = 0x9902;
/// Link state of the wireless headset, sent by the dock on its own.
pub const CMD_LINK: u16 = 0x2cb1;

pub struct Packet {
    /// 5a command, 5b response, 5c command without response, 5d indication
    pub kind: u8,
    pub cmd: u16,
    pub payload: Vec<u8>,
}

/// An output report asking for a record (`S` + name subscribes: the headset answers now and pushes changes).
pub fn subscribe(record: &str, token: u8) -> [u8; REPORT_LEN] {
    let mut body = vec![0x61, 0x00, b'S'];
    body.extend_from_slice(record.as_bytes());
    request(0x01, &body, token)
}

/// An output report reading a record once (`R` + name). The BT record answers only this, not a subscribe.
pub fn read_once(record: &str, token: u8) -> [u8; REPORT_LEN] {
    let mut body = vec![0x61, 0x00, b'R'];
    body.extend_from_slice(record.as_bytes());
    request(0x01, &body, token)
}

/// RACE command 0x000A: the headset's stored DSP parameters (Neap's DspParameter). Payload: op u16 BE (0 set,
/// 1 read), parameter u32 BE, value u32 BE (an f32 in dB). Byte 2 bit 0x80 asks a dock/transmitter to relay it to
/// the headset.
pub const CMD_DSP: u16 = 0x000A;
/// NVKey 0x5008, the playback input level before the EQ (Neap's loudness boost; factory -12 dB).
pub const DSP_INPUT_LEVEL: u32 = 0x0004_0000;

pub fn dsp(op: u16, param: u32, db: f32) -> [u8; REPORT_LEN] {
    let mut payload = op.to_be_bytes().to_vec();
    payload.extend_from_slice(&param.to_be_bytes());
    payload.extend_from_slice(&db.to_bits().to_be_bytes());
    let inner = (2 + payload.len()) as u16;
    let mut race = vec![0x05, 0x5a];
    race.extend_from_slice(&inner.to_le_bytes());
    race.extend_from_slice(&CMD_DSP.to_le_bytes());
    race.extend_from_slice(&payload);
    let mut report = [0u8; REPORT_LEN];
    report[0] = REPORT_OUT;
    report[1] = race.len() as u8;
    report[2] = 0x80;
    report[3..3 + race.len()].copy_from_slice(&race);
    report
}

/// The level in a 0x000A reply for `param`: payload = parameter u32 BE, value u32 BE (f32 bits).
pub fn dsp_level(payload: &[u8], param: u32) -> Option<f32> {
    let p: [u8; 8] = payload.try_into().ok()?;
    (u32::from_be_bytes(p[..4].try_into().ok()?) == param)
        .then(|| f32::from_bits(u32::from_be_bytes(p[4..].try_into().unwrap())))
        .filter(|v| v.is_finite())
}

/// An output report writing one key, as Swarm II does: `{"0xa20":"2"}`. The JSON must stay short enough to fit
/// one report (about 32 characters).
pub fn set_kvp(key: &str, value: &str, token: u8) -> [u8; REPORT_LEN] {
    let mut body = b"\xb7set_kvp\xff".to_vec();
    body.extend_from_slice(format!(r#"{{"0x{key}":"{value}"}}"#).as_bytes());
    request(0x03, &body, token)
}

/// Session byte of this process's requests, different per run: a request whose token bytes repeat a recent
/// one (as every run starting at the same values would) can go unanswered.
fn session() -> u8 {
    use std::sync::OnceLock;
    static SESSION: OnceLock<u8> = OnceLock::new();
    *SESSION.get_or_init(|| random_byte() | 0x10)
}

/// Good enough randomness for request tokens: clock nanoseconds mixed with the process id.
pub fn random_byte() -> u8 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let x = nanos ^ std::process::id().rotate_left(13);
    (x ^ (x >> 8) ^ (x >> 16)) as u8
}
/// RACE 0x9902 request: `48 <op> <session> <counter> <seq> 00*7 <body>`; the device echoes the token bytes.
fn request(op: u8, payload: &[u8], token: u8) -> [u8; REPORT_LEN] {
    let mut body = vec![0x48, op, session(), token, 0x01, 0, 0, 0, 0, 0, 0, 0];
    body.extend_from_slice(payload);
    let len = (body.len() + 2) as u16;
    let mut race = vec![0x05, 0x5a];
    race.extend_from_slice(&len.to_le_bytes());
    race.extend_from_slice(&CMD_KVP.to_le_bytes());
    race.extend_from_slice(&body);
    let mut report = [0u8; REPORT_LEN];
    report[0] = REPORT_OUT;
    report[1] = race.len() as u8;
    // report[2] = 0: target the dock, which relays record requests to the headset
    report[3..3 + race.len()].copy_from_slice(&race);
    report
}

/// The input side is a byte stream: a packet can start mid-report and span several reports.
#[derive(Default)]
pub struct Reassembler {
    buf: Vec<u8>,
}

impl Reassembler {
    pub fn feed(&mut self, report: &[u8]) -> Vec<Packet> {
        if report.len() < 3 || report[0] != REPORT_IN {
            return Vec::new();
        }
        let n = (report[1] as usize).min(report.len() - 3);
        let data = &report[3..3 + n];
        // The dock sometimes loses a report. Packets start at a report boundary, so a report that begins with a
        // packet header while an earlier packet is still incomplete means that packet lost a piece: drop it
        // instead of letting it swallow this one (JSON text never contains the 0x05 header byte).
        if data.len() >= 2 && data[0] == 0x05 && (0x5a..=0x5d).contains(&data[1]) {
            self.buf.clear();
        }
        self.buf.extend_from_slice(data);
        let mut packets = Vec::new();
        loop {
            // resync on the start of a packet: 05 followed by a type 5a..5d
            while !self.buf.is_empty()
                && !(self.buf[0] == 0x05
                    && self.buf.get(1).is_none_or(|t| (0x5a..=0x5d).contains(t)))
            {
                self.buf.remove(0);
            }
            if self.buf.len() < 6 {
                return packets;
            }
            let len = u16::from_le_bytes([self.buf[2], self.buf[3]]) as usize;
            if !(2..=1024).contains(&len) {
                self.buf.remove(0);
                continue;
            }
            if self.buf.len() < 4 + len {
                return packets;
            }
            let packet: Vec<u8> = self.buf.drain(..4 + len).collect();
            packets.push(Packet {
                kind: packet[1],
                cmd: u16::from_le_bytes([packet[4], packet[5]]),
                payload: packet[6..].to_vec(),
            });
        }
    }
}

/// The JSON documents in a 0x9902 reply or push, e.g. `{"UP":"GSI","KVP":{"240":"55"}}`.
pub fn documents(payload: &[u8]) -> Vec<serde_json::Value> {
    let Some(start) = payload.iter().position(|&b| b == b'{') else {
        return Vec::new();
    };
    serde_json::Deserializer::from_slice(&payload[start..])
        .into_iter()
        .map_while(Result::ok)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_matches_captured_layout() {
        let r = subscribe("GSI", 1);
        assert_eq!(
            &r[..9],
            &[0x06, 0x18, 0x00, 0x05, 0x5a, 0x14, 0x00, 0x02, 0x99]
        );
        assert_eq!(&r[24..27], b"GSI");
    }

    #[test]
    fn dsp_frames() {
        let r = dsp(0, DSP_INPUT_LEVEL, -6.0);
        assert_eq!(
            &r[..9],
            &[0x06, 0x10, 0x80, 0x05, 0x5a, 0x0c, 0x00, 0x0a, 0x00]
        );
        assert_eq!(&r[9..15], &[0, 0, 0, 4, 0, 0]);
        assert_eq!(
            f32::from_bits(u32::from_be_bytes(r[15..19].try_into().unwrap())),
            -6.0
        );
        let mut reply = DSP_INPUT_LEVEL.to_be_bytes().to_vec();
        reply.extend_from_slice(&(-12.0f32).to_bits().to_be_bytes());
        assert_eq!(dsp_level(&reply, DSP_INPUT_LEVEL), Some(-12.0));
    }

    #[test]
    fn reassembles_across_reports_and_finds_json() {
        let doc = br#"{"UP":"GSI","KVP":{"240":"55"}}"#;
        let mut payload = vec![
            0x58, 0x45, 0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0x61, 3, 0x62, 0x32, 0xef, 0x21, 1, 0xff,
        ];
        payload.extend_from_slice(doc);
        let mut race = vec![0x05, 0x5b];
        race.extend_from_slice(&((payload.len() + 2) as u16).to_le_bytes());
        race.extend_from_slice(&CMD_KVP.to_le_bytes());
        race.extend_from_slice(&payload);
        let mut stream = vec![0xaa]; // junk before the packet
        stream.extend_from_slice(&race);
        let mut r = Reassembler::default();
        let mut got = Vec::new();
        for chunk in stream.chunks(59) {
            let mut report = vec![REPORT_IN, chunk.len() as u8, 0];
            report.extend_from_slice(chunk);
            got.extend(r.feed(&report));
        }
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].cmd, CMD_KVP);
        assert_eq!(documents(&got[0].payload)[0]["KVP"]["240"], "55");

        // a lost middle report: the broken packet is dropped and the next one still comes through
        let chunks: Vec<&[u8]> = race.chunks(20).collect();
        let mut r = Reassembler::default();
        let mut got = Vec::new();
        for chunk in chunks
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, c)| c)
            .chain(race.chunks(59).collect::<Vec<_>>().iter())
        {
            let mut report = vec![REPORT_IN, chunk.len() as u8, 0];
            report.extend_from_slice(chunk);
            got.extend(r.feed(&report));
        }
        assert_eq!(got.len(), 1);
        assert_eq!(documents(&got[0].payload)[0]["KVP"]["240"], "55");
    }
}
