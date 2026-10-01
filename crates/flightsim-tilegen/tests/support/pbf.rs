//! Independent protobuf encoder for small, readable hostile/valid fixtures.
#![allow(dead_code)]

pub fn varint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    while value >= 128 {
        bytes.push(value.to_le_bytes()[0] | 0x80);
        value >>= 7;
    }
    bytes.push(value.to_le_bytes()[0]);
    bytes
}

pub fn integer(field: u32, value: u64) -> Vec<u8> {
    [varint(u64::from(field) << 3), varint(value)].concat()
}

pub fn bytes(field: u32, value: &[u8]) -> Vec<u8> {
    [
        varint((u64::from(field) << 3) | 2),
        varint(value.len() as u64),
        value.to_vec(),
    ]
    .concat()
}

pub fn zigzag(value: i64) -> u64 {
    u64::from_ne_bytes(((value << 1) ^ (value >> 63)).to_ne_bytes())
}

pub fn signed(field: u32, value: i64) -> Vec<u8> {
    integer(field, zigzag(value))
}

pub fn packed(field: u32, values: &[u64]) -> Vec<u8> {
    bytes(
        field,
        &values
            .iter()
            .flat_map(|value| varint(*value))
            .collect::<Vec<_>>(),
    )
}

pub fn deltas(field: u32, values: &[i64]) -> Vec<u8> {
    packed(
        field,
        &values
            .iter()
            .map(|value| zigzag(*value))
            .collect::<Vec<_>>(),
    )
}

pub fn node(id: i64, latitude: i64, longitude: i64, tags: &[(u64, u64)]) -> Vec<u8> {
    [
        signed(1, id),
        packed(2, &tags.iter().map(|x| x.0).collect::<Vec<_>>()),
        packed(3, &tags.iter().map(|x| x.1).collect::<Vec<_>>()),
        signed(8, latitude),
        signed(9, longitude),
    ]
    .concat()
}

pub fn dense(ids: &[i64], latitudes: &[i64], longitudes: &[i64], tags: &[u64]) -> Vec<u8> {
    [
        deltas(1, ids),
        deltas(8, latitudes),
        deltas(9, longitudes),
        packed(10, tags),
    ]
    .concat()
}

pub fn way(id: u64, refs: &[i64], tags: &[(u64, u64)]) -> Vec<u8> {
    [
        integer(1, id),
        packed(2, &tags.iter().map(|x| x.0).collect::<Vec<_>>()),
        packed(3, &tags.iter().map(|x| x.1).collect::<Vec<_>>()),
        deltas(8, refs),
    ]
    .concat()
}

pub fn relation(refs: &[i64], roles: &[u64], types: &[u64]) -> Vec<u8> {
    [
        integer(1, 30),
        packed(8, roles),
        deltas(9, refs),
        packed(10, types),
    ]
    .concat()
}

pub fn block(group: &[u8], extra: &[u8]) -> Vec<u8> {
    let strings = [
        "", "aeroway", "runway", "taxiway", "area", "yes", "width", "30",
    ];
    let table = strings
        .iter()
        .flat_map(|text| bytes(1, text.as_bytes()))
        .collect::<Vec<_>>();
    [bytes(1, &table), bytes(2, group), extra.to_vec()].concat()
}

pub fn frame(kind: &str, blob: &[u8]) -> Vec<u8> {
    let header = [bytes(1, kind.as_bytes()), integer(3, blob.len() as u64)].concat();
    let size = u32::try_from(header.len()).expect("small fixture header");
    [size.to_be_bytes().to_vec(), header, blob.to_vec()].concat()
}

pub fn pbf(block: &[u8]) -> Vec<u8> {
    let header = [bytes(4, b"OsmSchema-V0.6"), bytes(4, b"DenseNodes")].concat();
    [
        frame("OSMHeader", &bytes(1, &header)),
        frame("OSMData", &bytes(1, block)),
    ]
    .concat()
}

/// One uncompressed DEFLATE block inside a zlib envelope, independent of flate2.
pub fn zlib_stored(payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len()).expect("one small DEFLATE block");
    let mut bytes = vec![0x78, 0x01, 0x01];
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&(!length).to_le_bytes());
    bytes.extend_from_slice(payload);
    let (mut a, mut b) = (1_u32, 0_u32);
    for &value in payload {
        a = (a + u32::from(value)) % 65_521;
        b = (b + a) % 65_521;
    }
    bytes.extend_from_slice(&((b << 16) | a).to_be_bytes());
    bytes
}

pub fn valid_airport() -> Vec<u8> {
    let group = [
        bytes(1, &node(1, 350_000_000, 1_390_000_000, &[])),
        bytes(1, &node(2, 350_100_000, 1_390_000_000, &[])),
        bytes(1, &node(3, 1_000_000_000, 1_390_000_000, &[])),
        bytes(3, &way(10, &[1, 1], &[(1, 2), (6, 7)])),
        bytes(3, &way(11, &[1, 0], &[(1, 2)])),
        bytes(3, &way(12, &[1, 1], &[(1, 2), (4, 5)])),
        bytes(3, &way(13, &[1, 998], &[(1, 2)])),
        bytes(3, &way(14, &[1, 2], &[(1, 2)])),
        bytes(3, &way(15, &[1, 1], &[(1, 3)])),
    ]
    .concat();
    pbf(&block(&group, &[]))
}
