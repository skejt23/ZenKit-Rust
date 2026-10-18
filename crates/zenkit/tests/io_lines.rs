use zenkit::{ErrorKind, io::Reader};

const WHITESPACE: &[u8] = b" \t\r\n\x0b\x0c";

#[test]
fn mixed_terminators_skip_only_selected_bytes_and_keep_payload_raw() {
    let bytes = b" \tname\xff\r\n \tword\0 \rrest\n";
    let mut input = Reader::new(bytes);
    let first = input.line_then_ignore(WHITESPACE, 8, 3, "name").unwrap();
    assert_eq!(first, b" \tname\xff");
    assert_eq!(first.as_ptr(), bytes.as_ptr());
    assert_eq!(
        input.line_then_ignore(WHITESPACE, 4, 0, "word").unwrap(),
        b"word"
    );
    assert_eq!(
        input
            .take_bytes(2, "NUL preserves following whitespace")
            .unwrap(),
        b" \r"
    );
    assert_eq!(input.line_then_ignore(b"", 4, 0, "rest").unwrap(), b"rest");
    assert_eq!(input.remaining(), 0);
}

#[test]
fn empty_set_keeps_the_second_crlf_byte_and_lone_endings_are_valid() {
    let mut input = Reader::new(b"a\r\nb\nc\r");
    for expected in [b"a".as_slice(), b"", b"b", b"c"] {
        assert_eq!(input.line_then_ignore(b"", 1, 0, "line").unwrap(), expected);
    }
    assert_eq!(input.remaining(), 0);
}

#[test]
fn eof_without_a_terminator_and_repeated_empty_eof_succeed() {
    let mut input = Reader::new(b"last\xff");
    assert_eq!(
        input.line_then_ignore(WHITESPACE, 5, 0, "last").unwrap(),
        b"last\xff"
    );
    for _ in 0..3 {
        assert_eq!(
            input.line_then_ignore(WHITESPACE, 0, 0, "eof").unwrap(),
            b""
        );
        assert_eq!(input.position(), 5);
    }
}

#[test]
fn nul_lookahead_distinguishes_terminal_and_nonterminal_bytes() {
    for ignored in [b" ".as_slice(), b"\0", b" \0\0"] {
        let mut terminal = Reader::new(b"x\n\0");
        assert_eq!(
            terminal.line_then_ignore(ignored, 1, 1, "last").unwrap(),
            b"x"
        );
        assert_eq!(terminal.remaining(), 0);
        let mut internal = Reader::new(b"x\n\0z");
        assert_eq!(
            internal.line_then_ignore(ignored, 1, 0, "last").unwrap(),
            b"x"
        );
        assert_eq!(internal.take_bytes(2, "following field").unwrap(), b"\0z");
    }
    let mut empty_policy = Reader::new(b"x\n\0");
    empty_policy.line_then_ignore(b"", 1, 0, "last").unwrap();
    assert_eq!(empty_policy.u8("unread terminal NUL").unwrap(), 0);
    let mut nul_terminated = Reader::new(b"x\0 \t");
    nul_terminated
        .line_then_ignore(WHITESPACE, 1, 0, "last")
        .unwrap();
    assert_eq!(nul_terminated.take_bytes(2, "suffix").unwrap(), b" \t");
}

#[test]
fn both_budgets_are_exact_and_failure_restores_absolute_substream_cursor() {
    for (bytes, ignored, payload, suffix) in [
        (b"ab\n".as_slice(), b"".as_slice(), 1, 0),
        (b"ab", b"", 1, 0),
        (b"a\n  z", b" ", 1, 1),
        (b"a\n\0", b" ", 1, 0),
        (b"a\n \0", b" ", 1, 1),
    ] {
        let mut storage = b"prefix".to_vec();
        storage.extend_from_slice(bytes);
        storage.extend_from_slice(b"outside");
        let mut root = Reader::new(&storage);
        root.take_bytes(6, "prefix").unwrap();
        let mut input = root.substream(bytes.len(), "window").unwrap();
        let error = input
            .line_then_ignore(ignored, payload, suffix, "bounded line")
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::LimitExceeded);
        assert_eq!(error.offset, Some(6));
        assert_eq!(error.context, "bounded line");
        assert_eq!(input.position(), 6);
        assert_eq!(
            input.take_bytes(bytes.len(), "still intact").unwrap(),
            bytes
        );
    }
    let mut exact = Reader::new(b"a\n \0");
    assert_eq!(exact.line_then_ignore(b" ", 1, 2, "exact").unwrap(), b"a");
    assert_eq!(exact.remaining(), 0);
}

#[test]
fn maximum_limits_zero_payload_and_full_byte_set_are_supported() {
    let ignored: Vec<u8> = (0..=255).collect();
    let mut input = Reader::new(b"\n\xff\x80\0z");
    assert_eq!(
        input.line_then_ignore(&ignored, 0, 2, "empty").unwrap(),
        b""
    );
    assert_eq!(input.take_bytes(2, "NUL and suffix").unwrap(), b"\0z");
    let mut input = Reader::new(b"last\r \xff\n");
    assert_eq!(
        input
            .line_then_ignore(&ignored, usize::MAX, usize::MAX, "all")
            .unwrap(),
        b"last"
    );
    assert_eq!(input.remaining(), 0);
}

#[test]
fn substream_eof_is_the_boundary_for_terminal_nul_compatibility() {
    let mut parent = Reader::new(b"prex\n\0outside");
    parent.take_bytes(3, "prefix").unwrap();
    let mut window = parent.substream(3, "line window").unwrap();
    assert_eq!(window.line_then_ignore(b" ", 1, 1, "line").unwrap(), b"x");
    assert_eq!(window.position(), 6);
    assert_eq!(parent.take_bytes(7, "outside").unwrap(), b"outside");
}
