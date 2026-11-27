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

/// An output report writing one key, as Swarm II does: `{"0xa20":"2"}`. The JSON must stay short enough to fit
/// one report (about 32 characters).
pub fn set_kvp(key: &str, value: &str, token: u8) -> [u8; REPORT_LEN] {
    let mut body = b"\xb7set_kvp\xff".to_vec();
    body.extend_from_slice(format!(r#"{{"0x{key}":"{value}"}}"#).as_bytes());
    request(0x03, &body, token)
}

/// RACE 0x9902 request: `48 <op> <session> <counter> <seq> 00*7 <body>`; the device echoes the token bytes.
fn request(op: u8, payload: &[u8], token: u8) -> [u8; REPORT_LEN] {
    let mut body = vec![0x48, op, 0x5b, token, 0x01, 0, 0, 0, 0, 0, 0, 0];
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
        self.buf.extend_from_slice(&report[3..3 + n]);
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
    }
}
