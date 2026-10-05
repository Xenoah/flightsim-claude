use std::io::Write;
#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    pub method: u16,
    pub mode: u32,
    pub size: Option<u32>,
    pub flags: u16,
    pub compressed_payload: Option<Vec<u8>>,
}
impl Entry {
    pub fn new(name: &str, data: &[u8]) -> Self {
        Self {
            name: name.into(),
            data: data.to_vec(),
            method: 0,
            mode: 0o100644,
            size: None,
            flags: 0,
            compressed_payload: None,
        }
    }
}
fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_le_bytes());
}
pub fn zip(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = u32::try_from(out.len()).unwrap();
        let payload = if let Some(bytes) = &entry.compressed_payload {
            bytes.clone()
        } else if entry.method == 8 {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
            encoder.write_all(&entry.data).unwrap();
            encoder.finish().unwrap()
        } else {
            entry.data.clone()
        };
        let crc = rawzip::crc32(&entry.data);
        let size = entry
            .size
            .unwrap_or(u32::try_from(entry.data.len()).unwrap());
        let compressed = u32::try_from(payload.len()).unwrap();
        u32le(&mut out, 0x04034b50);
        u16le(&mut out, 20);
        u16le(&mut out, entry.flags);
        u16le(&mut out, entry.method);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u32le(&mut out, crc);
        u32le(&mut out, compressed);
        u32le(&mut out, size);
        u16le(&mut out, u16::try_from(entry.name.len()).unwrap());
        u16le(&mut out, 0);
        out.extend(entry.name.as_bytes());
        out.extend(payload);
        u32le(&mut central, 0x02014b50);
        u16le(&mut central, 0x0314);
        u16le(&mut central, 20);
        u16le(&mut central, entry.flags);
        u16le(&mut central, entry.method);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u32le(&mut central, crc);
        u32le(&mut central, compressed);
        u32le(&mut central, size);
        u16le(&mut central, u16::try_from(entry.name.len()).unwrap());
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u32le(&mut central, entry.mode << 16);
        u32le(&mut central, offset);
        central.extend(entry.name.as_bytes());
    }
    let offset = u32::try_from(out.len()).unwrap();
    let len = u32::try_from(central.len()).unwrap();
    out.extend(central);
    u32le(&mut out, 0x06054b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, u16::try_from(entries.len()).unwrap());
    u16le(&mut out, u16::try_from(entries.len()).unwrap());
    u32le(&mut out, len);
    u32le(&mut out, offset);
    u16le(&mut out, 0);
    out
}
