use history::*;
use zstd::zstd_safe::CParameter;

fn fixture() -> (Catalog, Vec<u8>) {
    let mut catalog = Catalog::standard(1);
    catalog.channels.truncate(1);
    let block = Block {
        catalog_version: 1,
        segment: 1,
        session_ref: DAY_HEADER_LEN as u64,
        frames: vec![Frame {
            sample_seq: 1,
            time_ms: 0,
            period_ms: 1,
            clock: ClockEvidence::default(),
            values: vec![None],
        }],
    };
    let encoded = block.encode_packet(&catalog, 3).unwrap();
    (catalog, decode_packet(&encoded).unwrap().0.payload.to_vec())
}

fn replace_body(payload: &[u8], data: &[u8]) -> Vec<u8> {
    let mut payload = payload[..59].to_vec();
    payload[54..58].copy_from_slice(&(data.len() as u32).to_le_bytes());
    payload.extend_from_slice(&zstd::bulk::compress(data, 3).unwrap());
    payload
}

fn parse(payload: &[u8], catalog: &Catalog) -> Result<Packet> {
    let mut encoded = vec![PacketTag::Block as u8];
    encoded.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    encoded.extend_from_slice(payload);
    encoded.extend_from_slice(&crc32c::crc32c(&encoded).to_le_bytes());
    decode_packet(&encoded)?.0.decode(Some(catalog))
}

#[test]
fn rejects_valid_crc_with_malformed_block_values_and_headers() {
    let (catalog, payload) = fixture();
    assert!(parse(&payload, &catalog).is_ok());
    for (data, reason) in [
        (&[0, 0, 1, 0x80][..], "nonzero bitmap padding"),
        (
            &[0, 0, 1, 1, 0x91, 0x4e],
            "noncanonical or overflowing varint",
        ),
        (&[0, 0, 1, 0, 0], "trailing payload bytes"),
        (&[0x80, 0, 0, 1, 0], "noncanonical or overflowing varint"),
        (&[0, 0, 0, 0], "block point identity or period"),
    ] {
        let payload = replace_body(&payload, data);
        assert!(
            matches!(parse(&payload, &catalog), Err(Error::Invalid(actual)) if actual == reason)
        );
    }
    let mut bad = payload.clone();
    bad[52..54].copy_from_slice(&(MAX_BLOCK_POINTS as u16 + 1).to_le_bytes());
    assert!(matches!(
        parse(&bad, &catalog),
        Err(Error::Limit("block point count"))
    ));
    let mut bad = payload.clone();
    bad[54..58].copy_from_slice(&(MAX_BLOCK_DECOMPRESSED as u32 + 1).to_le_bytes());
    assert!(matches!(
        parse(&bad, &catalog),
        Err(Error::Limit("block decompressed bytes"))
    ));
    let mut bad = payload.clone();
    bad[54..58].copy_from_slice(&5u32.to_le_bytes());
    assert!(matches!(
        parse(&bad, &catalog),
        Err(Error::Invalid("zstd frame content size"))
    ));
}

#[test]
fn rejects_zstd_trailing_data_overlarge_windows_and_extra_output() {
    let (catalog, payload) = fixture();
    let mut concatenated = payload.clone();
    concatenated.extend_from_slice(&payload[59..]);
    assert!(matches!(
        parse(&concatenated, &catalog),
        Err(Error::ZstdFrame)
    ));

    let mut compressor = zstd::bulk::Compressor::new(3).unwrap();
    compressor
        .set_parameter(CParameter::ContentSizeFlag(false))
        .unwrap();
    let mut compressed = compressor.compress(&[0, 0, 1, 0]).unwrap();
    assert_eq!(compressed[4] & 0x20, 0);
    compressed[5] = 10 << 3;
    let bad = [&payload[..59], &compressed].concat();
    assert!(matches!(
        parse(&bad, &catalog),
        Err(Error::Limit("zstd window bytes"))
    ));

    let compressed = compressor.compress(&[0, 0, 1, 0, 0]).unwrap();
    let bad = [&payload[..59], &compressed].concat();
    assert!(matches!(
        parse(&bad, &catalog),
        Err(Error::Invalid("extra zstd output"))
    ));
}
