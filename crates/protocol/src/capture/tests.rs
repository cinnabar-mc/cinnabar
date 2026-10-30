use super::*;

const HEADER: CaptureHeader = CaptureHeader {
    protocol_version: crate::PROTOCOL_VERSION,
    started_unix_ms: 1_760_000_000_123,
};

fn packet(
    direction: PacketDirection,
    packet_id: u32,
    elapsed_us: u64,
    body: &[u8],
) -> CaptureRecord {
    CaptureRecord::Packet {
        sequence: 1,
        direction,
        packet_id,
        elapsed_us,
        body: Bytes::copy_from_slice(body),
    }
}

fn encode(limit: u64, records: &[CaptureRecord]) -> (Vec<u8>, Vec<bool>) {
    let mut writer = CaptureWriter::with_limit(Vec::new(), HEADER, limit).unwrap();
    let accepted = records
        .iter()
        .map(|record| writer.write(record).unwrap())
        .collect();
    let written = writer.written();
    let bytes = writer.into_inner();
    assert_eq!(written, bytes.len() as u64);
    (bytes, accepted)
}

fn read(bytes: &[u8]) -> Result<CaptureFile, CaptureError> {
    read_capture(Bytes::copy_from_slice(bytes))
}

fn complete(records: &[CaptureRecord]) -> CaptureFile {
    CaptureFile {
        version: CAPTURE_VERSION,
        header: HEADER,
        records: records.to_vec(),
        cut_short: false,
    }
}

#[test]
fn records_round_trip_with_their_header_and_little_endian_layout() {
    let records = [
        packet(PacketDirection::Clientbound, 11, 0, &[1, 2, 3]),
        packet(PacketDirection::Serverbound, 0x90, 1_500, &[]),
        CaptureRecord::Gap {
            elapsed_us: 2_000,
            dropped: 7,
        },
        packet(PacketDirection::Clientbound, 0x3ff, u64::MAX, &[0xff; 300]),
    ];
    let (bytes, accepted) = encode(MAX_CAPTURE_BYTES, &records);
    assert_eq!(accepted, [true; 4]);
    assert_eq!(&bytes[..8], b"CNBRCAP2");
    assert_eq!(&bytes[8..10], &CAPTURE_VERSION.to_le_bytes());
    assert_eq!(&bytes[10..14], &crate::PROTOCOL_VERSION.to_le_bytes());
    assert_eq!(&bytes[14..22], &HEADER.started_unix_ms.to_le_bytes());
    let mut first = vec![0_u8];
    first.extend(11_u32.to_le_bytes());
    first.extend(0_u64.to_le_bytes());
    first.extend(11_u32.to_le_bytes());
    first.extend(1_u64.to_le_bytes());
    first.extend([1, 2, 3]);
    assert_eq!(&bytes[22..50], first.as_slice());
    let expected_len =
        CAPTURE_HEADER_BYTES + records.iter().map(CaptureRecord::encoded_len).sum::<u64>();
    assert_eq!(bytes.len() as u64, expected_len);
    assert_eq!(read(&bytes), Ok(complete(&records)));
}

#[test]
fn gap_records_count_dropped_packets_in_stream_order() {
    let records = [
        packet(PacketDirection::Clientbound, 11, 10, &[1]),
        CaptureRecord::Gap {
            elapsed_us: 20,
            dropped: u64::MAX,
        },
        packet(PacketDirection::Clientbound, 9, 30, &[2]),
    ];
    let (bytes, _) = encode(MAX_CAPTURE_BYTES, &records);
    assert_eq!(read(&bytes), Ok(complete(&records)));
    let mut writer = CaptureWriter::new(Vec::new(), HEADER).unwrap();
    assert!(
        writer
            .write(&CaptureRecord::Gap {
                elapsed_us: 1,
                dropped: 0
            })
            .is_err()
    );
}

#[test]
fn size_limit_ends_the_capture_with_one_final_marker_and_never_exceeds_it() {
    let body = [7_u8; 10];
    let one = RECORD_HEADER_BYTES + 8 + body.len() as u64;
    // Room for two packets and the final marker, but not a third packet.
    let limit = CAPTURE_HEADER_BYTES + 2 * one + RECORD_HEADER_BYTES + one - 1;
    let records: Vec<_> = (0..5)
        .map(|index| packet(PacketDirection::Clientbound, 9, index * 100, &body))
        .collect();
    let (bytes, accepted) = encode(limit, &records);
    assert_eq!(accepted, [true, true, false, false, false]);
    assert!(bytes.len() as u64 <= limit);
    assert_eq!(
        read(&bytes),
        Ok(complete(&[
            records[0].clone(),
            records[1].clone(),
            CaptureRecord::Truncated { elapsed_us: 200 }
        ]))
    );
    let exact = CAPTURE_HEADER_BYTES + 3 * one + RECORD_HEADER_BYTES;
    assert_eq!(encode(exact, &records).1, [true, true, true, false, false]);
    assert!(CaptureWriter::with_limit(Vec::new(), HEADER, CAPTURE_HEADER_BYTES).is_err());
}

/// A writer that stopped mid-record leaves a readable capture of the records before it.
#[test]
fn a_file_ending_inside_a_record_keeps_the_complete_records_and_is_cut_short() {
    let records = [
        packet(PacketDirection::Clientbound, 11, 0, &[1, 2, 3]),
        packet(PacketDirection::Clientbound, 9, 5, &[4, 5]),
    ];
    let (bytes, _) = encode(MAX_CAPTURE_BYTES, &records);
    let second = CAPTURE_HEADER_BYTES + records[0].encoded_len();
    for cut in 0..bytes.len() {
        let result = read(&bytes[..cut]);
        if (cut as u64) < CAPTURE_HEADER_BYTES {
            assert!(result.is_err(), "header cut at {cut}");
            continue;
        }
        let kept = usize::from(cut as u64 >= second);
        let boundary = cut as u64 == CAPTURE_HEADER_BYTES || cut as u64 == second;
        assert_eq!(
            result,
            Ok(CaptureFile {
                version: CAPTURE_VERSION,
                header: HEADER,
                records: records[..kept].to_vec(),
                cut_short: !boundary,
            }),
            "cut at {cut}"
        );
    }
}

#[test]
fn reader_rejects_malformed_records_and_records_after_the_final_marker() {
    let (bytes, _) = encode(
        MAX_CAPTURE_BYTES,
        &[packet(PacketDirection::Clientbound, 11, 0, &[1, 2, 3])],
    );
    let record = CAPTURE_HEADER_BYTES as usize;
    let mut version = bytes.clone();
    version[8] = 4;
    assert_eq!(read(&version), Err(CaptureError::Version(4)));
    let mut kind = bytes.clone();
    kind[record] = 9;
    assert_eq!(read(&kind), Err(CaptureError::Kind { record: 0, kind: 9 }));

    let (gap, _) = encode(
        MAX_CAPTURE_BYTES,
        &[CaptureRecord::Gap {
            elapsed_us: 4,
            dropped: 2,
        }],
    );
    let malformed_gap = CaptureError::Marker {
        record: 0,
        kind: "gap",
    };
    let mut named = gap.clone();
    named[record + 1] = 1;
    assert_eq!(read(&named), Err(malformed_gap.clone()));
    let mut empty = gap.clone();
    empty[record + RECORD_HEADER_BYTES as usize] = 0;
    assert_eq!(read(&empty), Err(malformed_gap));

    let mut after_end = Vec::new();
    let mut writer = CaptureWriter::with_limit(
        &mut after_end,
        HEADER,
        CAPTURE_HEADER_BYTES + RECORD_HEADER_BYTES,
    )
    .unwrap();
    assert!(
        !writer
            .write(&packet(PacketDirection::Clientbound, 11, 5, &[1]))
            .unwrap()
    );
    let mut named_end = after_end.clone();
    named_end[record + 1] = 1;
    assert_eq!(
        read(&named_end),
        Err(CaptureError::Marker {
            record: 0,
            kind: "final"
        })
    );
    after_end.extend_from_slice(&bytes[record..]);
    assert_eq!(read(&after_end), Err(CaptureError::AfterEnd { record: 1 }));
}

#[test]
fn captures_over_the_size_limit_are_rejected() {
    let (bytes, _) = encode(
        MAX_CAPTURE_BYTES,
        &[packet(PacketDirection::Clientbound, 11, 0, &[1, 2, 3])],
    );
    let len = bytes.len() as u64;
    let data = Bytes::from(bytes);
    assert!(read_capture_within(data.clone(), len).is_ok());
    assert_eq!(
        read_capture_within(data, len - 1),
        Err(CaptureError::TooLarge(len))
    );
}

#[test]
fn v1_streams_are_not_mistaken_for_v2() {
    let v1 = [11_u32.to_le_bytes(), 0_u32.to_le_bytes()].concat();
    assert!(!is_capture_v2(&v1));
    assert_eq!(read(&v1), Err(CaptureError::Magic));
}

#[test]
fn frozen_pick_round_trips_with_its_outgoing_packet_sequence() {
    let state = PickState {
        pick_frame: [1, 20],
        press_frame: [1, 21],
        transaction_frame: [1, 23],
        target: 42,
        last_movement_sequence: 90,
        applied_sequence: 95,
        remote_tick: 17,
        frame_alpha: Some(0.25),
        eye: [1.0, 2.0, 3.0],
        direction: [0.0, 0.0, -1.0],
        pick_box: Some(([-0.1, 1.0, -3.0], [0.1, 3.0, -2.0])),
        previous_position: [0.0, 1.0, -3.0],
        current_position: [0.0, 1.0, -2.0],
        interpolation_steps: 2,
        hit: [0.0, 2.0, -2.0],
        assisted: false,
    };
    let records = [
        CaptureRecord::Packet {
            sequence: 100,
            direction: PacketDirection::Serverbound,
            packet_id: 30,
            elapsed_us: 300,
            body: Bytes::from_static(&[1, 2]),
        },
        CaptureRecord::Pick {
            packet_sequence: 100,
            elapsed_us: 300,
            state,
        },
    ];
    let (encoded, _) = encode(MAX_CAPTURE_BYTES, &records);
    assert_eq!(read(&encoded), Ok(complete(&records)));
    let mut malformed = encoded;
    *malformed.last_mut().unwrap() = 8;
    assert!(matches!(
        read(&malformed),
        Err(CaptureError::Marker { kind: "pick", .. })
    ));
}

#[test]
fn legacy_v2_packets_remain_readable_without_capture_sequences() {
    let mut bytes = Vec::from(CAPTURE_MAGIC);
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(HEADER.protocol_version.to_le_bytes());
    bytes.extend(HEADER.started_unix_ms.to_le_bytes());
    write_record_header(&mut bytes, 0, 11, 25, 2).unwrap();
    bytes.extend([4, 5]);
    let file = read(&bytes).unwrap();
    assert_eq!(file.version, 2);
    assert_eq!(
        file.records,
        [CaptureRecord::Packet {
            sequence: 0,
            direction: PacketDirection::Clientbound,
            packet_id: 11,
            elapsed_us: 25,
            body: Bytes::from_static(&[4, 5])
        }]
    );
}

#[test]
fn size_limit_accepts_or_rejects_a_whole_packet_pick_pair() {
    let packet = packet(PacketDirection::Serverbound, 30, 100, &[4, 5]);
    let pick = CaptureRecord::Pick {
        elapsed_us: 100,
        packet_sequence: 1,
        state: PickState::default(),
    };
    let exact =
        CAPTURE_HEADER_BYTES + packet.encoded_len() + pick.encoded_len() + RECORD_HEADER_BYTES;
    for (limit, accepted) in [(exact, true), (exact - 1, false)] {
        let mut writer = CaptureWriter::with_limit(Vec::new(), HEADER, limit).unwrap();
        assert_eq!(writer.write_pair(&packet, Some(&pick)).unwrap(), accepted);
        let bytes = writer.into_inner();
        assert!(bytes.len() as u64 <= limit);
        let file = read(&bytes).unwrap();
        if accepted {
            assert_eq!(file.records, [packet.clone(), pick.clone()]);
        } else {
            assert_eq!(file.records, [CaptureRecord::Truncated { elapsed_us: 100 }]);
        }
    }
}

#[test]
fn capture_trace_resolves_delayed_packets_and_reuses_frame_storage() {
    let trace = PickTrace::default();
    trace.world(7, 13);
    trace.world(8, 11);
    // Apple's std mutex boxes its pthread lock on first use; that is setup, not frame storage.
    assert_eq!(trace.clock(), (0, None));
    let (_, allocations) = test_alloc::count(|| {
        trace.advance(2, 0.25);
        assert_eq!(trace.packet_sequence(7), 13);
        assert_eq!(trace.packet_sequence(8), 11);
        assert_eq!(trace.packet_sequence(0), 0);
        assert_eq!(trace.clock(), (2, Some(0.25)));
    });
    assert_eq!(allocations, 0);
}

#[test]
fn capture_correspondence_keeps_live_poses_and_releases_obsolete_history() {
    let trace = PickTrace::default();
    for sequence in 1..=1000 {
        trace.world(sequence, sequence + 100);
    }
    let (_, allocations) = test_alloc::count(|| {
        assert_eq!(trace.packet_sequence(10), 110);
        assert_eq!(trace.packet_sequence(900), 1000);
        trace.retire(900);
    });
    assert_eq!(allocations, 0);
    assert_eq!(trace.packet_sequence(9), 0);
    assert_eq!(trace.packet_sequence(10), 110);
    assert_eq!(trace.packet_sequence(901), 1001);
    // A later publication no longer references the departed actor's old movement.
    trace.retire(1000);
    trace.retire(1000);
    assert_eq!(trace.packet_sequence(10), 0);
    trace.world(1001, 1101);
    trace.stop();
    trace.world(1002, 1102);
    assert!(!trace.active());
    assert_eq!(trace.packet_sequence(1001), 0);
    assert_eq!(trace.packet_sequence(1002), 0);
}
