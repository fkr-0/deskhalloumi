//! Tests for the `unilii-lib` crate.  These are basic sanity checks
//! ensuring that the system monitoring functions can be invoked
//! without panicking.  Because the tests run in an isolated
//! container we do not assert on specific values; instead we simply
//! verify that calls succeed.

use deskhalloumi_lib::input;
use deskhalloumi_lib::process;
use deskhalloumi_lib::sysfs::backlight::BacklightDevice;
use deskhalloumi_lib::sysfs::power::PowerDevice;
use futures_util::StreamExt;

/// Ensure that the power device enumeration does not panic and
/// returns a vector (possibly empty) of devices.
#[tokio::test]
async fn test_power_device_enumeration() {
    let _ = PowerDevice::read_all()
        .await
        .expect("reading power devices");
}

/// Ensure that backlight devices can be read without panicking.
#[tokio::test]
async fn test_backlight_enumeration() {
    let _ = BacklightDevice::read_all()
        .await
        .expect("reading backlight devices");
}

/// Ensure keyboard discovery is safe even on containers without `/dev/input`.
#[test]
fn test_keyboard_scan_is_hardware_optional() {
    let stats = input::scan_keyboard_device_stats();
    assert!(stats.keyboard_candidates <= stats.total_devices);
}

/// Ensure that the process listener produces a stream that can be
/// polled once.  We only poll one element to avoid prolonged
/// execution.
#[tokio::test]
async fn test_process_listener() {
    let mut stream = process::listen_running_processes(std::time::Duration::from_secs(1));
    // Poll once.  If there are no processes the stream may return
    // `None` which is acceptable; the important part is that polling
    // does not panic.
    let _ = stream.next().await;
}
