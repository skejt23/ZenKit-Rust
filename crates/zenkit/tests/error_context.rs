use std::{error::Error as _, fmt, io};
use zenkit::{ContextError, Error, ErrorKind, io::Reader};

#[test]
fn runtime_labels_outlive_builders_and_recover_original_string_buffers() {
    let (error, resource_ptr, detail_ptr, resource_capacity, detail_capacity) = {
        let mut resource = String::with_capacity(96);
        resource.push_str("world 17");
        let mut detail = String::with_capacity(96);
        detail.push_str("loading section 23");
        let pointers = (resource.as_ptr(), detail.as_ptr());
        let capacities = (resource.capacity(), detail.capacity());
        let error = Reader::new(&[1]).u32("header").unwrap_err();
        (
            error.with_context(resource, Some(detail)),
            pointers.0,
            pointers.1,
            capacities.0,
            capacities.1,
        )
    };
    assert_eq!(error.resource(), "world 17");
    assert_eq!(error.detail(), Some("loading section 23"));
    assert_eq!(error.inner().kind, ErrorKind::UnexpectedEof);
    assert_eq!(error.inner().offset, Some(0));
    assert_eq!(error.inner().context, "header");
    let (inner, resource, detail) = error.into_parts();
    let detail = detail.unwrap();
    assert_eq!(
        (resource.as_ptr(), detail.as_ptr()),
        (resource_ptr, detail_ptr)
    );
    assert_eq!(
        (resource.capacity(), detail.capacity()),
        (resource_capacity, detail_capacity)
    );
    assert_eq!(inner.kind, ErrorKind::UnexpectedEof);
}

#[test]
fn none_empty_unicode_and_nul_are_distinct_and_unmodified() {
    for detail in [None, Some(""), Some("section\0 Łódź 東京")] {
        let resource = "asset\0 café";
        let error = ContextError::new(17, resource.to_owned(), detail.map(str::to_owned));
        assert_eq!(error.resource(), resource);
        assert_eq!(error.detail(), detail);
        let display = match detail {
            None => format!("{resource}: 17"),
            Some(detail) => format!("{resource} [context: {detail}]: 17"),
        };
        assert_eq!(error.to_string(), display);
        assert_eq!(error.into_inner(), 17);
    }
}

#[derive(Debug)]
struct DeviceFailure(u32);
impl fmt::Display for DeviceFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "device failure {}", self.0)
    }
}
impl std::error::Error for DeviceFailure {}

#[test]
fn nested_typed_and_io_sources_survive_without_slicing_or_flattening() {
    let io = io::Error::other(DeviceFailure(23));
    let inner = Error::io(io, "read device").with_context("file 5".into(), None);
    let error = ContextError::new(inner, "preview 9".into(), Some("thumbnail".into()));
    let first = error
        .source()
        .unwrap()
        .downcast_ref::<ContextError>()
        .unwrap();
    let second = first.source().unwrap().downcast_ref::<Error>().unwrap();
    let third = second
        .source()
        .unwrap()
        .downcast_ref::<io::Error>()
        .unwrap();
    assert_eq!(
        third
            .get_ref()
            .unwrap()
            .downcast_ref::<DeviceFailure>()
            .unwrap()
            .0,
        23
    );
    assert!(error.to_string().contains("device failure 23"));
    let recovered = error.into_inner().into_inner();
    assert_eq!(recovered.kind, ErrorKind::Io);
    assert!(recovered.source().unwrap().is::<io::Error>());
}

#[test]
fn host_and_raw_virtual_paths_remain_in_the_inner_error() {
    use zenkit::vfs::{HostOptions, Vfs};
    let missing = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("Cargo.toml")
        .join("impossible-child");
    let mut vfs = Vfs::new(Default::default());
    let inner = vfs
        .mount_host(&missing, b"target\xff", HostOptions::default())
        .unwrap_err();
    assert_eq!(inner.host_path(), Some(missing.as_path()));
    assert_eq!(inner.virtual_path(), Some(b"target\xff".as_slice()));
    let original_kind = inner.kind;
    let original_offset = inner.offset;
    let original_source = inner.source().map(ToString::to_string);
    let error = inner.with_context("host snapshot".into(), Some("selected tree".into()));
    assert_eq!(error.inner().kind, original_kind);
    assert_eq!(error.inner().offset, original_offset);
    assert_eq!(error.inner().host_path(), Some(missing.as_path()));
    assert_eq!(error.inner().virtual_path(), Some(b"target\xff".as_slice()));
    assert_eq!(
        error.inner().source().map(ToString::to_string),
        original_source
    );
}

#[test]
fn borrowed_non_error_values_work_without_a_static_bound() {
    let text = String::from("caller-owned cause");
    let error = ContextError::new(text.as_str(), "operation".into(), None);
    assert_eq!(*error.inner(), text);
    assert_eq!(error.to_string(), "operation: caller-owned cause");
    assert_eq!(error.into_inner(), text.as_str());
}

#[test]
fn map_err_thread_transfer_and_boxed_sources_keep_typed_errors() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<ContextError>();
    send_sync::<ContextError<io::Error>>();
    let error = Reader::new(&[])
        .u16("version")
        .map_err(|error| error.with_context(format!("asset {}", 4), None))
        .unwrap_err();
    let inner = std::thread::spawn(move || error.into_inner())
        .join()
        .unwrap();
    assert_eq!(inner.kind, ErrorKind::UnexpectedEof);
    let error = ContextError::new(Box::new(inner), "boxed cause".into(), None);
    assert_eq!(error.inner().kind, ErrorKind::UnexpectedEof);
    assert!(error.source().unwrap().is::<Box<Error>>());
}

#[test]
fn display_propagates_sink_failure_without_losing_error_data() {
    struct RefusingSink;
    impl fmt::Write for RefusingSink {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }
    let error = Error::new(ErrorKind::InvalidData, 81, "field")
        .with_context("resource".into(), Some("detail".into()));
    assert!(fmt::write(&mut RefusingSink, format_args!("{error}")).is_err());
    assert_eq!(error.inner().offset, Some(81));
    assert_eq!(error.detail(), Some("detail"));
}
