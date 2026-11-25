use history::*;

const START: u64 = DAY_HEADER_LEN as u64;
const CLOCK: ClockEvidence = ClockEvidence {
    wall_ms: Some(-30),
    tick_ms: Some(u64::MAX),
    awake_ms: Some(0),
};

fn frame(catalog: &Catalog, seq: u64, time_ms: i64) -> Frame {
    Frame {
        sample_seq: seq,
        time_ms,
        period_ms: 250,
        clock: CLOCK,
        values: vec![None; catalog.channels.len()],
    }
}

fn session(id: u8) -> Packet {
    Packet::Marker(Marker::Session(Session {
        session_id: [id; 16],
        boot_id: Some([2; 16]),
    }))
}

fn append(ctx: &mut FileContext, offset: &mut u64, record: &Packet, catalog: Option<&Catalog>) {
    let bytes = record.encode(catalog).unwrap();
    let (packet, n) = decode_packet(&bytes).unwrap();
    assert_eq!(ctx.decode_at(*offset, &packet).unwrap(), *record);
    *offset += n as u64;
}

#[test]
fn day_and_boundary_validate_version_crc_and_extent() {
    let header = DayHeader {
        utc_day: -1,
        day_id: [0xf1; 16],
    };
    let encoded = header.encode();
    assert_eq!(&encoded[..10], b"RHH\0\x01\x00\xff\xff\xff\xff");
    assert_eq!(DayHeader::decode(&encoded).unwrap(), header);
    assert!(matches!(
        DayHeader::decode(&encoded[..29]),
        Err(Error::Truncated)
    ));
    let mut changed = encoded;
    changed[4] = 2;
    assert!(matches!(
        DayHeader::decode(&changed),
        Err(Error::UnsupportedVersion(2))
    ));
    changed[4] = 1;
    changed[7] ^= 1;
    assert!(matches!(DayHeader::decode(&changed), Err(Error::Checksum)));

    let boundary = PublishBoundary {
        day_id: header.day_id,
        offset: START,
    };
    let bytes = boundary.encode();
    assert_eq!(PublishBoundary::decode(&bytes).unwrap(), boundary);
    assert!(PublishBoundary::decode(&[&bytes[..], &[0]].concat()).is_err());
    assert!(
        PublishBoundary::decode(
            &PublishBoundary {
                offset: START - 1,
                ..boundary
            }
            .encode()
        )
        .is_err()
    );
}

#[test]
fn framing_is_bounded_and_crc_covers_header_and_payload() {
    let roll = Packet::Roll(b"123456789".to_vec());
    let encoded = roll.encode(None).unwrap();
    assert_eq!(&encoded[..5], &[6, 9, 0, 0, 0]);
    assert_eq!(
        &encoded[14..],
        &crc32c::crc32c(&encoded[..14]).to_le_bytes()
    );
    assert_eq!(crc32c::crc32c(b"123456789"), 0xe306_9283);
    let concatenated = [&encoded[..], &encoded[..]].concat();
    let (packet, consumed) = decode_packet(&concatenated).unwrap();
    assert_eq!(packet.decode(None).unwrap(), roll);
    assert_eq!(consumed, encoded.len());
    assert_eq!(packet_len(&encoded[..5]).unwrap(), encoded.len());
    for byte in &mut [0, 1, 2, 3, 4, 5, 7, 9, 12, 15] {
        let mut broken = encoded.clone();
        broken[*byte] ^= 1;
        assert!(decode_packet(&broken).is_err());
    }
    for size in 0..encoded.len() {
        assert!(matches!(
            decode_packet(&encoded[..size]),
            Err(Error::Truncated)
        ));
    }
    assert!(matches!(
        packet_len(&[6, 0xff, 0xff, 0xff, 0xff]),
        Err(Error::Limit(_))
    ));
    assert!(matches!(
        packet_len(&[8, 0, 0, 0, 0]),
        Err(Error::UnknownTag(8))
    ));
    assert!(matches!(
        packet_len(&[5, 0, 0, 0, 0]),
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        Packet::Roll(vec![0; MAX_PACKET_LEN - PACKET_OVERHEAD])
            .encode(None)
            .unwrap()
            .len(),
        MAX_PACKET_LEN
    );
    assert!(matches!(
        Packet::Roll(vec![0; MAX_PACKET_LEN]).encode(None),
        Err(Error::Limit(_))
    ));
}

#[test]
fn catalog_and_frame_preserve_sparse_native_width_values() {
    let catalog = Catalog::standard(7);
    assert_eq!(catalog.channels.len(), 13);
    assert_eq!(catalog.channels[0].quantize(42.125), Some(4213));
    assert_eq!(catalog.channels[0].quantize(100.001), None);
    assert_eq!(catalog.channels[0].dequantize(10_001), None);
    assert_eq!(catalog.channels[3].quantize(-100.0), Some(0));
    assert_eq!(catalog.channels[3].dequantize(0), Some(-100.0));
    assert_eq!(catalog.channels[4].quantize(f64::NAN), None);
    assert_eq!(catalog.channels[4].quantize(f64::INFINITY), None);
    assert_eq!(catalog.channels[4].quantize(-0.01), None);
    assert_eq!(catalog.channels[6].quantize(u64::MAX as f64), None);
    let mut sample = frame(&catalog, 123, -50);
    sample.values[0] = Some(10_000);
    sample.values[12] = Some(u64::MAX);
    let record = Packet::Frame(sample.clone());
    let encoded = record.encode(Some(&catalog)).unwrap();
    let (packet, _) = decode_packet(&encoded).unwrap();
    assert!(matches!(packet.decode(None), Err(Error::Catalog)));
    assert_eq!(packet.decode(Some(&catalog)).unwrap(), record);
    assert_eq!(&packet.payload[45..49], &[1, 16, 0x10, 0x27]);
    let mut malformed = packet.payload.to_vec();
    malformed[46] |= 0x80;
    assert!(matches!(
        RawPacket {
            payload: &malformed,
            ..packet
        }
        .decode(Some(&catalog)),
        Err(Error::Invalid("nonzero bitmap padding"))
    ));
    malformed[20] |= 0x08;
    assert!(matches!(
        RawPacket {
            payload: &malformed,
            ..packet
        }
        .decode(Some(&catalog)),
        Err(Error::Invalid("reserved clock flags"))
    ));
    sample.values[0] = Some(10_001);
    assert!(Packet::Frame(sample).encode(Some(&catalog)).is_err());
    let mut duplicate = catalog.clone();
    duplicate.channels.push(duplicate.channels[0].clone());
    assert!(Packet::Catalog(duplicate).encode(None).is_err());
}

#[test]
fn validates_file_local_context_and_wal_only_records() {
    let catalog = Catalog::standard(1);
    let record = Packet::Frame(frame(&catalog, 0, 123));
    let bytes = record.encode(Some(&catalog)).unwrap();
    let (raw, _) = decode_packet(&bytes).unwrap();
    let mut day = FileContext::new(FileKind::Day);
    let mut wal = FileContext::new(FileKind::Wal);
    let mut offset = START;
    assert!(matches!(wal.decode_at(offset, &raw), Err(Error::Session)));
    assert!(matches!(
        day.decode_at(offset, &raw),
        Err(Error::Invalid(_))
    ));
    append(&mut wal, &mut offset, &session(1), None);
    assert!(matches!(wal.decode_at(offset, &raw), Err(Error::Catalog)));
    append(
        &mut wal,
        &mut offset,
        &Packet::Catalog(catalog.clone()),
        None,
    );
    append(&mut wal, &mut offset, &record, Some(&catalog));
    assert_eq!(wal.current_catalog().unwrap().version, 1);
    assert_eq!(wal.current_session().unwrap().0, START);
    let marker = Packet::Marker(Marker::Event(Event {
        kind: MarkerKind::Continuation,
        event_id: [3; 16],
        segment: 7,
        session_ref: START,
        sample_seq: None,
        clock: CLOCK,
    }));
    append(&mut wal, &mut offset, &marker, None);
    let bytes = marker.encode(None).unwrap();
    let (raw, _) = decode_packet(&bytes).unwrap();
    let mut day_offset = START;
    append(&mut day, &mut day_offset, &session(1), None);
    assert!(matches!(
        day.decode_at(day_offset, &raw),
        Err(Error::Invalid(_))
    ));
    let mut event = Event {
        kind: MarkerKind::ClockSegment,
        event_id: [4; 16],
        segment: 8,
        session_ref: day_offset,
        sample_seq: Some(0),
        clock: CLOCK,
    };
    let bytes = Packet::Marker(Marker::Event(event)).encode(None).unwrap();
    let (raw, _) = decode_packet(&bytes).unwrap();
    assert!(matches!(
        day.decode_at(day_offset, &raw),
        Err(Error::Session)
    ));
    event.session_ref = START;
    append(
        &mut day,
        &mut day_offset,
        &Packet::Marker(Marker::Event(event)),
        None,
    );
    let gap = Gap {
        from_ms: -250,
        to_ms: -40,
        reason: GapReason::RecorderOverrun,
        segment: 8,
        event_id: [10; 16],
    };
    append(&mut day, &mut day_offset, &Packet::Gap(gap), None);
    assert!(
        Packet::Gap(Gap {
            from_ms: gap.to_ms,
            ..gap
        })
        .encode(None)
        .is_err()
    );
}

#[test]
fn block_stores_sparse_columns_full_range_deltas_and_last_clock() {
    let catalog = Catalog::standard(9);
    let mut frames = vec![
        frame(&catalog, 0, i64::MIN),
        frame(&catalog, 1, -1),
        frame(&catalog, 8, 0),
        frame(&catalog, u64::MAX, i64::MAX),
    ];
    frames[0].values[6] = Some(u64::MAX);
    frames[2].values[6] = Some(0);
    frames[3].values[6] = Some(u64::MAX);
    frames[0].values[10] = Some(0);
    frames[2].values[10] = Some(u64::MAX);
    let mut block = Block {
        catalog_version: catalog.version,
        segment: 12,
        session_ref: START,
        frames,
    };
    let bytes = block
        .encode_packet(&catalog, DEFAULT_COMPRESSION_LEVEL)
        .unwrap();
    let (raw, _) = decode_packet(&bytes).unwrap();
    let header = raw.block_header().unwrap().unwrap();
    assert_eq!(
        (header.first_seq, header.last_seq, header.count),
        (0, u64::MAX, 4)
    );
    assert_eq!(
        (header.first_time_ms, header.last_time_ms),
        (i64::MIN, i64::MAX)
    );
    assert_eq!(header.last_clock, CLOCK);
    assert!(matches!(
        raw.decode(Some(&Catalog::standard(8))),
        Err(Error::Catalog)
    ));
    let mut ctx = FileContext::new(FileKind::Day);
    let mut offset = START;
    assert!(matches!(ctx.decode_at(offset, &raw), Err(Error::Session)));
    append(&mut ctx, &mut offset, &session(1), None);
    assert!(matches!(ctx.decode_at(offset, &raw), Err(Error::Catalog)));
    append(
        &mut ctx,
        &mut offset,
        &Packet::Catalog(catalog.clone()),
        None,
    );
    for sample in &mut block.frames[..3] {
        sample.clock = ClockEvidence::default();
    }
    assert_eq!(ctx.decode_at(offset, &raw).unwrap(), Packet::Block(block));
}

#[test]
fn splits_blocks_at_point_and_byte_limits() {
    let catalog = Catalog::standard(5);
    let block = Block {
        catalog_version: 5,
        segment: 7,
        session_ref: START,
        frames: (0..=MAX_BLOCK_POINTS)
            .map(|i| frame(&catalog, i as u64, i as i64))
            .collect(),
    };
    assert!(matches!(
        block.encode_packet(&catalog, 3),
        Err(Error::Limit(_))
    ));
    let packets = block.encode_split(&catalog, 3).unwrap();
    assert_eq!(packets.len(), 2);
    assert_eq!(
        decode_packet(&packets[0])
            .unwrap()
            .0
            .block_header()
            .unwrap()
            .unwrap()
            .count as usize,
        MAX_BLOCK_POINTS
    );
    let dense = Catalog {
        version: 17,
        channels: (0..MAX_CHANNELS)
            .map(|i| Channel {
                key: format!("column{i}"),
                unit: "B".into(),
                encoding: ColumnEncoding::Rate,
            })
            .collect(),
    };
    let mut rng = 12u64;
    let block = Block {
        catalog_version: dense.version,
        segment: 1,
        session_ref: START,
        frames: (0..MAX_BLOCK_POINTS)
            .map(|i| {
                let mut frame = frame(&dense, i as u64, i as i64);
                for value in &mut frame.values {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    *value = Some(rng);
                }
                frame
            })
            .collect(),
    };
    assert!(matches!(
        block.encode_packet(&dense, 3),
        Err(Error::Limit("block decompressed bytes"))
    ));
    let packets = block.encode_split(&dense, 3).unwrap();
    assert!(packets.len() > 4);
    let mut decoded_count = 0;
    for bytes in packets {
        assert!(bytes.len() <= MAX_PACKET_LEN);
        let (raw, _) = decode_packet(&bytes).unwrap();
        let Packet::Block(decoded) = raw.decode(Some(&dense)).unwrap() else {
            panic!("expected BLOCK")
        };
        for frame in decoded.frames {
            let expected = &block.frames[decoded_count];
            assert_eq!(
                (frame.sample_seq, frame.time_ms, frame.values),
                (
                    expected.sample_seq,
                    expected.time_ms,
                    expected.values.clone()
                )
            );
            decoded_count += 1;
        }
    }
    assert_eq!(decoded_count, MAX_BLOCK_POINTS);
}
