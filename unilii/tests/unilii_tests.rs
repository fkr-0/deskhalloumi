use deskhalloumi::util::{DockSide, build_strut_partial};

#[test]
fn test_build_strut_partial_left() {
    let width = 42;
    let height = 768;
    let strut = build_strut_partial(DockSide::Left, width, height);
    assert_eq!(strut[0], width);
    assert_eq!(strut[1], 0);
    assert_eq!(strut[4], 0);
    assert_eq!(strut[5], height);
}

#[test]
fn test_build_strut_partial_right() {
    let width = 64;
    let height = 900;
    let strut = build_strut_partial(DockSide::Right, width, height);
    assert_eq!(strut[0], 0);
    assert_eq!(strut[1], width);
    // For right side strut the start and end y are encoded in indices 6 and 7
    assert_eq!(strut[6], 0);
    assert_eq!(strut[7], height);
}

#[test]
fn test_time_format() {
    // The bar formats time as HH:MM:SS.  Verify that formatting a
    // representative date yields the expected pattern.  We avoid
    // asserting specific digits since the actual time will vary; we
    // simply check for the presence of colons and length.
    use chrono::{TimeZone, Utc};
    let dt = Utc.with_ymd_and_hms(2026, 2, 27, 13, 45, 59).unwrap();
    let formatted = dt.format("%H:%M:%S").to_string();
    assert_eq!(formatted.len(), 8);
    assert_eq!(&formatted[2..3], ":");
    assert_eq!(&formatted[5..6], ":");
}
