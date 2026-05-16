use geulos_proto::codec::{decode_frame, encode_frame, DecodeError};

#[test]
fn encode_decode_round_trip() {
    let body = br#"{"kind":"Hello"}"#.to_vec();
    let encoded = encode_frame(&body);
    // 4바이트 길이 + body
    assert_eq!(encoded.len(), 4 + body.len());

    let mut buf = encoded.as_slice();
    let decoded = decode_frame(&mut buf).expect("should decode");
    assert_eq!(decoded, body);
    assert_eq!(buf.len(), 0, "all consumed");
}

#[test]
fn decode_two_frames_in_one_buffer() {
    let a = encode_frame(b"first");
    let b = encode_frame(b"second");
    let mut combined: Vec<u8> = Vec::new();
    combined.extend_from_slice(&a);
    combined.extend_from_slice(&b);
    let mut slice = combined.as_slice();
    let d1 = decode_frame(&mut slice).unwrap();
    let d2 = decode_frame(&mut slice).unwrap();
    assert_eq!(d1, b"first");
    assert_eq!(d2, b"second");
}

#[test]
fn decode_incomplete_returns_incomplete_error() {
    // 길이 헤더만 있고 body 부족.
    let buf = [0u8, 0u8, 0u8, 10u8]; // 길이=10이지만 body 0바이트
    let mut slice = buf.as_slice();
    let err = decode_frame(&mut slice).unwrap_err();
    assert!(matches!(err, DecodeError::Incomplete));
}

#[test]
fn decode_too_short_for_length_returns_incomplete() {
    let buf = [0u8, 0u8]; // 4바이트 헤더도 부족
    let mut slice = buf.as_slice();
    assert!(matches!(decode_frame(&mut slice), Err(DecodeError::Incomplete)));
}

#[test]
fn encode_length_is_big_endian() {
    let body = vec![0u8; 256];
    let encoded = encode_frame(&body);
    assert_eq!(encoded[0], 0);
    assert_eq!(encoded[1], 0);
    assert_eq!(encoded[2], 1); // 256 = 0x00000100
    assert_eq!(encoded[3], 0);
}
