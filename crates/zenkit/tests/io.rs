use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use zenkit::{
    ErrorKind,
    io::{LineEnding, Reader, TextEncoding, Writer, read_bounded},
};

#[test]
fn primitive_wire_values_and_signed_extremes() {
    let bytes = [
        0xff, 0x80, 0xff, 0xff, 0, 0x80, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0x80, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0x80,
    ];
    let mut r = Reader::new(&bytes);
    assert_eq!(r.u8("u8").unwrap(), u8::MAX);
    assert_eq!(r.i8("i8").unwrap(), i8::MIN);
    assert_eq!(r.u16("u16").unwrap(), u16::MAX);
    assert_eq!(r.i16("i16").unwrap(), i16::MIN);
    assert_eq!(r.u32("u32").unwrap(), u32::MAX);
    assert_eq!(r.i32("i32").unwrap(), i32::MIN);
    assert_eq!(r.u64("u64").unwrap(), u64::MAX);
    assert_eq!(r.i64("i64").unwrap(), i64::MIN);
    let mut w = Writer::new(bytes.len());
    w.u8(u8::MAX, "u8").unwrap();
    w.i8(i8::MIN, "i8").unwrap();
    w.u16(u16::MAX, "u16").unwrap();
    w.i16(i16::MIN, "i16").unwrap();
    w.u32(u32::MAX, "u32").unwrap();
    w.i32(i32::MIN, "i32").unwrap();
    w.u64(u64::MAX, "u64").unwrap();
    w.i64(i64::MIN, "i64").unwrap();
    assert_eq!(w.as_slice(), bytes);
}

#[test]
fn floating_point_bit_patterns_survive_without_normalization() {
    for bits in [
        0,
        0x80000000,
        0x3f800000,
        0x7f800000,
        0xff800000,
        0x7fc12345,
        1,
        u32::MAX,
    ] {
        let data = bits.to_le_bytes();
        let mut r = Reader::new(&data);
        assert_eq!(r.f32("f32").unwrap().to_bits(), bits);
        let mut w = Writer::new(4);
        w.f32(f32::from_bits(bits), "f32").unwrap();
        assert_eq!(w.as_slice(), data);
    }
    for bits in [
        0,
        0x8000000000000000,
        0x3ff0000000000000,
        0x7ff0000000000000,
        0xfff0000000000000,
        0x7ff8123456789abc,
        1,
        u64::MAX,
    ] {
        let data = bits.to_le_bytes();
        let mut r = Reader::new(&data);
        assert_eq!(r.f64("f64").unwrap().to_bits(), bits);
        let mut w = Writer::new(8);
        w.f64(f64::from_bits(bits), "f64").unwrap();
        assert_eq!(w.as_slice(), data);
    }
}

#[test]
fn all_primitive_short_reads_are_atomic() {
    macro_rules! short {
        ($method:ident,$size:literal) => {
            for length in 0..$size {
                let data = vec![0xff; length];
                let mut r = Reader::new(&data);
                let e = r.$method("field").unwrap_err();
                assert_eq!(e.kind, ErrorKind::UnexpectedEof);
                assert_eq!(e.offset, Some(0));
                assert_eq!(r.position(), 0);
            }
        };
    }
    short!(u8, 1);
    short!(i8, 1);
    short!(u16, 2);
    short!(i16, 2);
    short!(u32, 4);
    short!(i32, 4);
    short!(u64, 8);
    short!(i64, 8);
    short!(f32, 4);
    short!(f64, 8);
}

#[test]
fn bounded_seek_and_nested_substream_offsets() {
    let mut r = Reader::new(b"0123456789");
    r.take_bytes(2, "prefix").unwrap();
    let mut child = r.substream(6, "child").unwrap();
    child.take_bytes(2, "prefix").unwrap();
    let mut nested = child.substream(3, "nested").unwrap();
    assert_eq!(nested.position(), 4);
    assert_eq!(nested.stream_position().unwrap(), 0);
    assert_eq!(nested.seek(SeekFrom::End(-1)).unwrap(), 2);
    assert_eq!(nested.position(), 6);
    assert_eq!(nested.u16("short").unwrap_err().offset, Some(6));
    assert_eq!(nested.position(), 6);
    for offset in [
        SeekFrom::Start(4),
        SeekFrom::Start(u64::MAX),
        SeekFrom::Current(i64::MIN),
        SeekFrom::Current(i64::MAX),
        SeekFrom::End(1),
    ] {
        let e = nested.seek_to(offset, "seek").unwrap_err();
        assert_eq!(e.offset, Some(6));
        assert_eq!(nested.position(), 6);
    }
    assert_eq!(nested.seek(SeekFrom::Start(0)).unwrap(), 0);
    assert_eq!(nested.take_bytes(3, "all").unwrap(), b"456");
    assert_eq!(child.position(), 7);
    assert_eq!(r.position(), 8);
    assert!(r.substream(3, "fail").is_err());
    assert_eq!(r.position(), 8);
    assert!(r.take_bytes(usize::MAX, "overflow").is_err());
    assert_eq!(r.position(), 8);
}

#[test]
fn raw_strings_and_exact_line_endings() {
    let mut r = Reader::new(b"a\xff\0\r\n lone\nCR\r\r\nlast\r\n");
    assert_eq!(r.terminated(0, 2, "name").unwrap(), b"a\xff");
    assert_eq!(r.line(LineEnding::CrLf, 0, "empty").unwrap(), b"");
    assert_eq!(
        r.line(LineEnding::CrLf, 32, "line").unwrap(),
        b" lone\nCR\r"
    );
    assert_eq!(r.line(LineEnding::Lf, 10, "line").unwrap(), b"last\r");
    assert_eq!(r.remaining(), 0);
    let mut w = Writer::new(64);
    w.terminated(b"a\xff", 0, 2, "name").unwrap();
    w.line(b"", LineEnding::CrLf, 0, "empty").unwrap();
    w.line(b" lone\nCR\r", LineEnding::CrLf, 32, "line")
        .unwrap();
    assert_eq!(w.as_slice(), b"a\xff\0\r\n lone\nCR\r\r\n");
}

#[test]
fn missing_endings_limits_and_ambiguous_partial_crlf() {
    for (bytes, limit, kind) in [
        (b"".as_slice(), 0, ErrorKind::UnexpectedEof),
        (b"a", 0, ErrorKind::LimitExceeded),
        (b"ab", 2, ErrorKind::UnexpectedEof),
        (b"abc\0", 2, ErrorKind::LimitExceeded),
    ] {
        let mut r = Reader::new(bytes);
        assert_eq!(r.terminated(0, limit, "name").unwrap_err().kind, kind);
        assert_eq!(r.position(), 0);
    }
    let mut r = Reader::new(b"ab\0");
    assert_eq!(r.terminated(0, 2, "name").unwrap(), b"ab");
    for (bytes, limit, kind) in [
        (b"\r".as_slice(), 0, ErrorKind::UnexpectedEof),
        (b"\n", 0, ErrorKind::LimitExceeded),
        (b"a\r", 1, ErrorKind::UnexpectedEof),
        (b"a\rx", 1, ErrorKind::LimitExceeded),
        (b"a\r\n", 0, ErrorKind::LimitExceeded),
    ] {
        let mut r = Reader::new(bytes);
        assert_eq!(
            r.line(LineEnding::CrLf, limit, "line").unwrap_err().kind,
            kind,
            "{bytes:?}"
        );
        assert_eq!(r.position(), 0);
    }
    assert_eq!(
        Reader::new(b"\n").line(LineEnding::Lf, 0, "empty").unwrap(),
        b""
    );
    assert_eq!(
        Reader::new(b"\r\n")
            .line(LineEnding::CrLf, 0, "empty")
            .unwrap(),
        b""
    );
    assert_eq!(
        Reader::new(b"x\0")
            .terminated(0, usize::MAX, "name")
            .unwrap(),
        b"x"
    );
}

#[test]
fn strict_text_is_borrowed_atomic_and_reports_invalid_byte() {
    let data = b"00a\xc3\xa9\xff";
    let mut root = Reader::new(data);
    root.take_bytes(2, "prefix").unwrap();
    let mut r = root.substream(4, "text").unwrap();
    let e = r.text(3, 3, TextEncoding::Ascii, "ascii").unwrap_err();
    assert_eq!(e.offset, Some(3));
    assert_eq!(r.position(), 2);
    let e = r.text(4, 4, TextEncoding::Utf8, "utf8").unwrap_err();
    assert_eq!(e.offset, Some(5));
    assert_eq!(r.position(), 2);
    let text = r.text(3, 3, TextEncoding::Utf8, "utf8").unwrap();
    assert_eq!(text, "aé");
    assert_eq!(text.as_ptr(), data[2..].as_ptr());
    let e = Reader::new(b"a\xc3")
        .text(2, 2, TextEncoding::Utf8, "truncated")
        .unwrap_err();
    assert_eq!(e.offset, Some(1));
    let mut r = Reader::new(b"abc");
    assert_eq!(
        r.text(3, 2, TextEncoding::Ascii, "limit").unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(r.position(), 0);
    assert_eq!(
        Reader::new(b"\0\t")
            .text(2, 2, TextEncoding::Ascii, "controls")
            .unwrap(),
        "\0\t"
    );
}

#[test]
fn writes_and_seeks_are_atomic_at_limits() {
    let mut w = Writer::new(5);
    w.bytes(b"abc", "data").unwrap();
    let before = w.as_slice().to_vec();
    for ending in [LineEnding::Lf, LineEnding::CrLf] {
        assert!(w.line(b"xx", ending, 2, "line").is_err());
        assert_eq!(w.as_slice(), before);
        assert_eq!(w.position(), 3);
    }
    assert!(w.terminated(b"xx", 0, 2, "name").is_err());
    assert!(w.u32(0, "value").is_err());
    assert_eq!(w.as_slice(), before);
    for offset in [
        SeekFrom::Start(4),
        SeekFrom::Current(-4),
        SeekFrom::End(i64::MAX),
    ] {
        assert!(w.seek_to(offset, "seek").is_err());
        assert_eq!(w.position(), 3);
    }
    w.seek(SeekFrom::Start(1)).unwrap();
    w.u16(0x0504, "patch").unwrap();
    assert_eq!(w.as_slice(), b"a\x04\x05");
    w.bytes(b"de", "append").unwrap();
    assert_eq!(w.as_slice(), b"a\x04\x05de");
    w.seek(SeekFrom::Start(0)).unwrap();
    assert_eq!(w.into_inner(), b"a\x04\x05de");
    assert!(Writer::new(0).bytes(b"x", "zero").is_err());
    assert!(Writer::new(0).bytes(b"", "zero").is_ok());
}

#[test]
fn embedded_terminators_are_rejected_without_mutation() {
    let mut w = Writer::new(64);
    w.bytes(b"prefix", "prefix").unwrap();
    assert_eq!(
        w.terminated(b"a\0b", 0, 3, "name").unwrap_err().kind,
        ErrorKind::InvalidData
    );
    assert_eq!(
        w.line(b"a\nb", LineEnding::Lf, 3, "line").unwrap_err().kind,
        ErrorKind::InvalidData
    );
    assert_eq!(
        w.line(b"a\r\nb", LineEnding::CrLf, 4, "line")
            .unwrap_err()
            .kind,
        ErrorKind::InvalidData
    );
    assert_eq!(
        w.line(b"abcd", LineEnding::Lf, 3, "line").unwrap_err().kind,
        ErrorKind::LimitExceeded
    );
    assert_eq!(w.position(), 6);
    assert_eq!(w.as_slice(), b"prefix");
}

#[test]
fn standard_io_traits_keep_streaming_semantics_and_error_source() {
    let mut r = Reader::new(b"abc");
    let mut buf = [9; 5];
    assert_eq!(r.read(&mut buf).unwrap(), 3);
    assert_eq!(buf, [b'a', b'b', b'c', 9, 9]);
    assert_eq!(r.read(&mut buf).unwrap(), 0);
    r.rewind().unwrap();
    assert!(r.read_exact(&mut buf).is_err());
    assert_eq!(r.position(), 3);
    r.rewind().unwrap();
    assert_eq!(r.fill_buf().unwrap(), b"abc");
    r.consume(1);
    assert_eq!(r.fill_buf().unwrap(), b"bc");
    let mut w = Writer::new(4);
    w.write_all(b"abc").unwrap();
    let e = w.write_all(b"de").unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::Other);
    assert!(
        e.get_ref()
            .unwrap()
            .downcast_ref::<zenkit::Error>()
            .is_some()
    );
    assert_eq!(w.as_slice(), b"abc");
    assert_eq!(
        w.seek(SeekFrom::Start(5)).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(w.stream_position().unwrap(), 3);
    w.flush().unwrap();
}

#[test]
fn bounded_external_short_reads_interrupts_and_errors() {
    struct Short<'a> {
        bytes: &'a [u8],
        interrupt: bool,
    }
    impl Read for Short<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.interrupt {
                self.interrupt = false;
                return Err(io::ErrorKind::Interrupted.into());
            }
            let n = out.len().min(self.bytes.len()).min(1);
            out[..n].copy_from_slice(&self.bytes[..n]);
            self.bytes = &self.bytes[n..];
            Ok(n)
        }
    }
    assert_eq!(
        read_bounded(
            Short {
                bytes: b"abc",
                interrupt: true
            },
            3
        )
        .unwrap(),
        b"abc"
    );
    assert_eq!(
        read_bounded(
            Short {
                bytes: b"abcd",
                interrupt: false
            },
            3
        )
        .unwrap_err()
        .kind,
        ErrorKind::LimitExceeded
    );
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "test failure",
            ))
        }
    }
    let e = read_bounded(Broken, 5).unwrap_err();
    assert!(std::error::Error::source(&e).is_some());
    assert_eq!(e.offset, None);
    assert_eq!(read_bounded(b"".as_slice(), 0).unwrap(), b"");
    assert!(read_bounded(b"x".as_slice(), 0).is_err());
}

#[test]
fn debug_output_is_compact_and_omits_payloads() {
    let data = vec![b'x'; 100_000];
    let r = Reader::new(&data);
    let mut w = Writer::default();
    w.bytes(&data, "data").unwrap();
    for text in [format!("{r:?}"), format!("{w:?}")] {
        assert!(text.len() < 128);
        assert!(!text.contains("xxx"));
    }
}

#[test]
fn formatted_trait_output_can_fail_after_committing_fragments() {
    struct Parts;
    impl std::fmt::Display for Parts {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("abc")?;
            f.write_str("def")
        }
    }
    let mut w = Writer::new(4);
    assert!(write!(&mut w, "{Parts}").is_err());
    assert_eq!(w.as_slice(), b"abc");
    assert_eq!(w.position(), 3);
    assert_eq!(w.bytes(b"xx", "full").unwrap_err().offset, None);
    let mut r = Reader::new(b"abc");
    assert_eq!(r.fixed_bytes(2, 3, "field").unwrap(), b"ab");
    let mut rest = String::new();
    r.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "c");
}
