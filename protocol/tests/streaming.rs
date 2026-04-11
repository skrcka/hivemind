//! Streaming-decoder tests: feed bytes in arbitrary chunkings and verify the
//! decoder produces the same frames as the all-at-once case. This is the test
//! that catches partial-read bugs in `SerialTransport` / `TcpTransport`.

use hivemind_protocol::{
    decode_frame, encode_frame, Envelope, FrameDecoder, OracleToLegion, PROTOCOL_VERSION,
};

fn make_frame(msg: OracleToLegion) -> Vec<u8> {
    let env = Envelope::new("drone-01", 1, msg);
    encode_frame(&env).unwrap()
}

#[test]
fn version_constant_pinned() {
    assert_eq!(PROTOCOL_VERSION, 1);
}

#[test]
fn frame_ends_in_zero_delimiter() {
    let frame = make_frame(OracleToLegion::Heartbeat);
    assert_eq!(frame.last(), Some(&0));
}

#[test]
fn cobs_body_never_contains_zero() {
    let frame = make_frame(OracleToLegion::Heartbeat);
    let body = &frame[..frame.len() - 1];
    for (i, &b) in body.iter().enumerate() {
        assert_ne!(b, 0, "zero byte at position {i} in COBS-encoded body");
    }
}

#[test]
fn single_frame_byte_by_byte() {
    let frame = make_frame(OracleToLegion::Heartbeat);
    let mut decoder = FrameDecoder::new();
    let mut decoded_bodies: Vec<Vec<u8>> = Vec::new();
    for &b in &frame {
        if let Some(body) = decoder.push(b) {
            decoded_bodies.push(body);
        }
    }
    assert_eq!(decoded_bodies.len(), 1);
    let env: Envelope<OracleToLegion> = decode_frame(&decoded_bodies[0]).unwrap();
    assert_eq!(env.msg, OracleToLegion::Heartbeat);
}

#[test]
fn three_frames_concatenated_at_once() {
    let f1 = make_frame(OracleToLegion::Heartbeat);
    let f2 = make_frame(OracleToLegion::Proceed {
        sortie_id: "s".into(),
        expected_step_index: 1,
    });
    let f3 = make_frame(OracleToLegion::ReturnToBase {
        reason: "test".into(),
    });

    let mut combined = Vec::new();
    combined.extend_from_slice(&f1);
    combined.extend_from_slice(&f2);
    combined.extend_from_slice(&f3);

    let mut decoder = FrameDecoder::new();
    let bodies = decoder.push_slice(&combined);

    assert_eq!(bodies.len(), 3);
    assert!(matches!(
        decode_frame::<OracleToLegion>(&bodies[0]).unwrap().msg,
        OracleToLegion::Heartbeat
    ));
    assert!(matches!(
        decode_frame::<OracleToLegion>(&bodies[1]).unwrap().msg,
        OracleToLegion::Proceed {
            expected_step_index: 1,
            ..
        }
    ));
    assert!(matches!(
        decode_frame::<OracleToLegion>(&bodies[2]).unwrap().msg,
        OracleToLegion::ReturnToBase { .. }
    ));
}

#[test]
fn split_at_every_possible_boundary() {
    // Build two frames concatenated.
    let f1 = make_frame(OracleToLegion::Heartbeat);
    let f2 = make_frame(OracleToLegion::Proceed {
        sortie_id: "s".into(),
        expected_step_index: 7,
    });
    let mut combined = Vec::new();
    combined.extend_from_slice(&f1);
    combined.extend_from_slice(&f2);

    // Try every split point — the decoder should produce exactly 2 frames
    // regardless of where the read boundary lands.
    for split in 1..combined.len() {
        let mut decoder = FrameDecoder::new();
        let mut all = decoder.push_slice(&combined[..split]);
        all.extend(decoder.push_slice(&combined[split..]));
        assert_eq!(
            all.len(),
            2,
            "split at {split}: produced {} frames, want 2",
            all.len()
        );
        let m1: Envelope<OracleToLegion> = decode_frame(&all[0]).unwrap();
        let m2: Envelope<OracleToLegion> = decode_frame(&all[1]).unwrap();
        assert!(matches!(m1.msg, OracleToLegion::Heartbeat));
        assert!(matches!(
            m2.msg,
            OracleToLegion::Proceed {
                expected_step_index: 7,
                ..
            }
        ));
    }
}

#[test]
fn empty_frames_are_ignored() {
    let mut decoder = FrameDecoder::new();
    // Three back-to-back delimiters with no payload bytes between them.
    let bodies = decoder.push_slice(&[0, 0, 0]);
    assert!(bodies.is_empty());
    assert_eq!(decoder.buffered(), 0);
}

#[test]
fn corrupt_garbage_then_valid_frame() {
    let valid = make_frame(OracleToLegion::Heartbeat);
    let mut decoder = FrameDecoder::new();

    // Inject some garbage bytes that aren't a valid COBS-encoded postcard
    // message, ended by a delimiter.
    let bad = decoder.push_slice(&[0xFF, 0xFE, 0xFD, 0x00]);
    // The decoder still emits a "frame" — it's just a buffer of bytes.
    // It's the codec layer's job to decide it's invalid.
    assert_eq!(bad.len(), 1);
    let bad_decode: Result<Envelope<OracleToLegion>, _> = decode_frame(&bad[0]);
    assert!(bad_decode.is_err(), "garbage should fail to decode");

    // The next valid frame should still decode correctly — the decoder is
    // resynchronised by the delimiter.
    let good = decoder.push_slice(&valid);
    assert_eq!(good.len(), 1);
    let env: Envelope<OracleToLegion> = decode_frame(&good[0]).unwrap();
    assert_eq!(env.msg, OracleToLegion::Heartbeat);
}

#[test]
fn reset_clears_partial_state() {
    let mut decoder = FrameDecoder::new();
    decoder.push_slice(&[0xAB, 0xCD]);
    assert_eq!(decoder.buffered(), 2);
    decoder.reset();
    assert_eq!(decoder.buffered(), 0);
    // After reset, a fresh frame still decodes.
    let frame = make_frame(OracleToLegion::Heartbeat);
    let bodies = decoder.push_slice(&frame);
    assert_eq!(bodies.len(), 1);
    let env: Envelope<OracleToLegion> = decode_frame(&bodies[0]).unwrap();
    assert_eq!(env.msg, OracleToLegion::Heartbeat);
}

#[test]
fn version_field_propagates() {
    let env = Envelope::new("drone-01", 42, OracleToLegion::Heartbeat);
    assert_eq!(env.v, PROTOCOL_VERSION);
    assert!(env.version_matches());
    let frame = encode_frame(&env).unwrap();
    let body = &frame[..frame.len() - 1];
    let decoded: Envelope<OracleToLegion> = decode_frame(body).unwrap();
    assert_eq!(decoded.v, PROTOCOL_VERSION);
    assert_eq!(decoded.ts_ms, 42);
    assert_eq!(decoded.drone_id, "drone-01");
}
