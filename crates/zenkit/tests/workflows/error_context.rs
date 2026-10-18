//! Add application labels while keeping inspectable parser and device failures.
use std::error::Error as _;
use zenkit::{ContextError, ErrorKind, io::Reader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let asset_name = format!("asset {}", 17);
    let error = Reader::new(&[1])
        .u32("header")
        .map_err(|error| error.with_context(asset_name, Some("building preview".into())))
        .unwrap_err();
    assert_eq!(error.inner().kind, ErrorKind::UnexpectedEof);
    assert_eq!(error.inner().offset, Some(0));
    assert!(error.source().unwrap().is::<zenkit::Error>());
    println!("{error}");

    // Box the host error to keep the outer Result small.
    let source = Box::new(std::io::Error::other("device disconnected"));
    let error = ContextError::new(source, "preview device".into(), None);
    let (source, resource, detail) = error.into_parts();
    assert_eq!(source.kind(), std::io::ErrorKind::Other);
    assert_eq!(resource, "preview device");
    assert_eq!(detail, None);
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
