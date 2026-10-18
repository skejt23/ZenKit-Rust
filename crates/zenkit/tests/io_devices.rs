use std::{
    error::Error as _,
    fs::{self, OpenOptions},
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    time::{SystemTime, UNIX_EPOCH},
};
use zenkit::{
    ErrorKind,
    io::{Reader, Writer, read_bounded},
};

// A caller-owned non-seekable device: short reads, interruption and a stable fault.
struct Source {
    bytes: &'static [u8],
    position: usize,
    interrupt: bool,
    fail_at: usize,
}
impl Read for Source {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        if self.position == self.fail_at {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "device fault",
            ));
        }
        let count = output
            .len()
            .min(2)
            .min(self.bytes.len() - self.position)
            .min(self.fail_at - self.position);
        output[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}
fn source() -> Source {
    Source {
        bytes: b"abcdefgh",
        position: 0,
        interrupt: true,
        fail_at: usize::MAX,
    }
}

#[test]
fn short_reads_and_interruption_work_and_original_errors_survive() {
    let mut device = source();
    assert_eq!(read_bounded(&mut device, 8).unwrap(), b"abcdefgh");
    assert_eq!(device.position, 8);
    device.position = 1;
    device.fail_at = 4;
    let error = read_bounded(&mut device, 8).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Io);
    assert_eq!(error.offset, None);
    let cause = error.source().unwrap().downcast_ref::<io::Error>().unwrap();
    assert_eq!(cause.kind(), io::ErrorKind::ConnectionReset);
    assert_eq!(device.position, 4);
    device.fail_at = usize::MAX;
    assert_eq!(read_bounded(&mut device, 4).unwrap(), b"efgh");
}

#[test]
fn exact_limit_empty_and_over_limit_have_explicit_device_consumption() {
    assert!(read_bounded(io::empty(), 0).unwrap().is_empty());
    for (limit, consumed, success) in [(0, 1, false), (7, 8, false), (8, 8, true), (9, 8, true)] {
        let mut device = source();
        let result = read_bounded(&mut device, limit);
        assert_eq!(device.position, consumed);
        if success {
            assert_eq!(result.unwrap(), b"abcdefgh");
        } else {
            assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded);
        }
    }
}

#[test]
fn framed_snapshot_starts_at_current_position_and_leaves_borrowed_device_suffix() {
    let mut device = Cursor::new(b"prefix\x78\x56\x34\x12suffix");
    device.seek(SeekFrom::Start(6)).unwrap();
    let bytes = read_bounded((&mut device).take(4), 4).unwrap();
    assert_eq!(device.position(), 10);
    let mut input = Reader::new(&bytes);
    assert_eq!(input.u32("value").unwrap(), 0x12345678);
    input.seek(SeekFrom::Start(0)).unwrap();
    assert_eq!(device.position(), 10);
    let mut suffix = String::new();
    device.read_to_string(&mut suffix).unwrap();
    assert_eq!(suffix, "suffix");
    assert_eq!(input.u32("again").unwrap(), 0x12345678);
}

fn output() -> Writer {
    let mut writer = Writer::new(4);
    writer.u32(0x12345678, "value").unwrap();
    writer
}

#[test]
fn borrowed_fixed_output_preserves_surroundings_and_reports_partial_exhaustion() {
    let writer = output();
    let mut storage = *b"abcdefghij";
    {
        let mut device = Cursor::new(storage.as_mut_slice());
        device.seek(SeekFrom::Start(2)).unwrap();
        device.write_all(writer.as_slice()).unwrap();
        assert_eq!(device.position(), 6);
        device.seek(SeekFrom::Start(8)).unwrap();
        assert_eq!(
            device.write_all(writer.as_slice()).unwrap_err().kind(),
            io::ErrorKind::WriteZero
        );
        assert_eq!(device.position(), 10);
        device.seek(SeekFrom::Start(0)).unwrap();
        device.write_all(b"!").unwrap();
    }
    assert_eq!(storage, *b"!b\x78\x56\x34\x12gh\x78\x56");
    assert_eq!(writer.as_slice(), b"\x78\x56\x34\x12");
}

#[test]
fn borrowed_vector_can_overwrite_grow_and_be_used_after_publication() {
    let mut storage = b"prefix.old".to_vec();
    {
        let mut device = Cursor::new(&mut storage);
        device.seek(SeekFrom::Start(7)).unwrap();
        device.write_all(output().as_slice()).unwrap();
        device.write_all(b"suffix").unwrap();
        device.seek(SeekFrom::Start(6)).unwrap();
        device.write_all(b"!").unwrap();
    }
    assert_eq!(storage, b"prefix!\x78\x56\x34\x12suffix");
    storage.push(b'?');
    assert_eq!(storage.last(), Some(&b'?'));
}

struct Sink {
    bytes: Vec<u8>,
    interrupt: bool,
    stop: usize,
    zero: bool,
    flushes: usize,
}
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        if self.bytes.len() == self.stop {
            return if self.zero {
                Ok(0)
            } else {
                Err(io::ErrorKind::BrokenPipe.into())
            };
        }
        let count = bytes.len().min(2).min(self.stop - self.bytes.len());
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Err(io::Error::other("flush fault"))
    }
}
#[test]
fn external_publication_handles_short_writes_but_is_not_atomic_or_implicitly_flushed() {
    for (stop, zero) in [(usize::MAX, false), (3, false), (3, true)] {
        let mut sink = Sink {
            bytes: vec![],
            interrupt: true,
            stop,
            zero,
            flushes: 0,
        };
        let writer = output();
        let result = sink.write_all(writer.as_slice());
        if stop == usize::MAX {
            result.unwrap();
            assert_eq!(sink.bytes, writer.as_slice());
            assert_eq!(sink.flushes, 0);
            assert!(sink.flush().is_err());
            assert_eq!(sink.flushes, 1);
        } else {
            assert_eq!(
                result.unwrap_err().kind(),
                if zero {
                    io::ErrorKind::WriteZero
                } else {
                    io::ErrorKind::BrokenPipe
                }
            );
            assert_eq!(sink.bytes, &writer.as_slice()[..3]);
        }
        assert_eq!(writer.as_slice(), b"\x78\x56\x34\x12");
    }
}

#[test]
fn owned_snapshot_survives_source_changes_closure_removal_and_thread_move() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("zenkit-owned-{}-{stamp}.bin", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.write_all(output().as_slice()).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let bytes = read_bounded(&mut file, 4).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"xxxx").unwrap();
    drop(file);
    fs::remove_file(path).unwrap();
    std::thread::spawn(move || {
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.u32("snapshot").unwrap(), 0x12345678);
        reader.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(reader.u32("again").unwrap(), 0x12345678);
    })
    .join()
    .unwrap();
}
