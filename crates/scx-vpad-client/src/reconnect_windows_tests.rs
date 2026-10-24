use super::*;

#[test]
#[ignore = "run explicitly against the previous service, before updating"]
fn windows_old_service_keeps_output_when_reconnect_is_unavailable() {
    assert!(!VirtualPad::reconnect_supported().unwrap());
    let mut pad = VirtualPad::create_profile(Profile::DualSense).unwrap();
    let identity = pad.instance_id().to_owned();
    assert_eq!(
        pad.reconnect().unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    assert!(device_present(&identity));
    let mut reader = GamepadReader::<64>::open(&pad).unwrap();
    let state = InputState {
        buttons: 1,
        left_x: 12345,
        ..InputState::NEUTRAL
    };
    let mut report = reader.read_after(2000, || pad.submit(&state)).unwrap();
    for _ in 0..32 {
        if report[1] == 176 && report[8] == 0x28 {
            break;
        }
        report = reader.read(2000).unwrap();
    }
    assert_eq!(report[1], 176);
    assert_eq!(report[8], 0x28);
}

fn retired(instance: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while device_present(instance) {
        assert!(Instant::now() < deadline, "owned lease did not retire");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires installed reconnect-capable service; close games first"]
fn windows_reconnect_is_pipe_bound_rotates_secret_and_throttles() {
    let mut victim = VirtualPad::create_profile(Profile::DualSense).unwrap();
    let original = victim.instance_id().to_owned();
    let original_lease = victim.lease;
    let old_secret = Capability::new(*victim.capability.bytes());
    let outsider = connect().unwrap();
    assert!(exchange(
        &outsider,
        Request {
            op: OP_RECONNECT,
            profile: 0,
            lease: victim.lease
        }
    )
    .is_err());
    assert!(device_present(&original));
    victim.neutral().unwrap();
    let attacker = VirtualPad::create_profile(Profile::DualShock4).unwrap();
    let own = attacker.instance_id().to_owned();
    assert!(exchange(
        &attacker.pipe,
        Request {
            op: OP_RECONNECT,
            profile: 0,
            lease: victim.lease
        }
    )
    .is_err());
    drop(attacker);
    retired(&own);
    assert!(device_present(&original));
    victim.reconnect().unwrap();
    assert_eq!(victim.lease, original_lease);
    assert_eq!(victim.instance_id(), original);
    assert_ne!(victim.capability, old_secret, "capability not rotated");
    assert!(
        victim.reconnect().is_err(),
        "rapid repeated reconnection accepted"
    );
    assert!(device_present(&original));
    let state = InputState {
        buttons: 1,
        left_x: 12345,
        ..InputState::NEUTRAL
    };
    let mut reader = GamepadReader::<64>::open(&victim).unwrap();
    let mut report = reader.read_after(2000, || victim.submit(&state)).unwrap();
    for _ in 0..32 {
        if report[1] == 176 && report[8] == 0x28 {
            break;
        }
        report = reader.read(2000).unwrap();
    }
    assert_eq!(report[1], 176);
    assert_eq!(report[8], 0x28);
    drop(reader);
    drop(victim);
    retired(&original);
    println!("PASS: foreign/new pipe refused, owner preserved, capability rotated, rate limit keeps live input, lease cleanup");
}
