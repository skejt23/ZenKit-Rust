//! Build, serialize and read raw values, then explicitly request a geometry operation.
use zenkit::{
    io::{Reader, Writer},
    values::{Date, Mat3, Obb, ObbLimits, Vec3},
};

fn main() -> zenkit::Result<()> {
    let date = Date {
        year: 2026,
        month: 10,
        day: 14,
        reserved: 0x1234,
        ..Date::default()
    };
    let bounds = Obb {
        center: Vec3::from([-10.0, -20.0, -30.0]),
        axes: Mat3::IDENTITY.columns,
        half_width: Vec3::from([1.0, 2.0, 3.0]),
        children: vec![Obb::default()],
    };
    let limits = ObbLimits { nodes: 2, depth: 2 };
    let mut output = Writer::new(140);
    date.write(&mut output, "date")?;
    bounds.write(&mut output, limits, "bounds")?;
    let mut input = Reader::new(output.as_ref());
    assert_eq!(Date::read(&mut input, "date")?, date);
    let restored = Obb::read(&mut input, limits, "bounds")?;
    assert_eq!(restored, bounds);
    assert_eq!(input.remaining(), 0);
    assert_eq!(restored.as_aabb()?.max, Vec3::from([-9.0, -18.0, -27.0]));
    println!(
        "Preserved date metadata and {} child; bounds {:?}",
        restored.children.len(),
        restored.as_aabb()?
    );
    Ok(())
}

#[test]
fn workflow() {
    main().unwrap();
}
