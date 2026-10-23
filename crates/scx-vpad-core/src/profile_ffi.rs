//! C entry points for immutable per-device profile selection.
use super::*;

const UNSUPPORTED: u32 = 5;
fn profile(raw: u32) -> Result<Profile, u32> {
    Profile::try_from(raw)
        .ok()
        .filter(|p| p.is_supported())
        .ok_or(UNSUPPORTED)
}

#[unsafe(no_mangle)]
pub extern "C" fn scx_vpad_profile_report_size(raw: u32) -> usize {
    profile(raw).map_or(0, Profile::report_size)
}

/// # Safety
/// `length` points to writable usize storage. Returned bytes are immutable/static.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_report_descriptor_for_profile(
    raw: u32,
    length: *mut usize,
) -> *const u8 {
    if length.is_null() {
        return core::ptr::null();
    }
    let descriptor = profile(raw).ok().and_then(profiles::descriptor);
    // SAFETY: the C caller owns the non-null output slot.
    unsafe {
        core::ptr::write_unaligned(length, descriptor.map_or(0, |d| d.len()));
    }
    descriptor.map_or(core::ptr::null(), |d| d.as_ptr())
}

/// # Safety
/// Input is readable for length; output writable for capacity. Overlap is allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_encode_report_for_profile(
    raw: u32,
    input: *const u8,
    length: usize,
    output: *mut u8,
    capacity: usize,
) -> u32 {
    if input.is_null() || output.is_null() {
        return STATUS_INVALID_POINTER;
    }
    let p = match profile(raw) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if p.report_size() == 0 {
        return UNSUPPORTED;
    }
    if length != INPUT_PACKET_SIZE {
        return STATUS_INVALID_LENGTH;
    }
    if capacity < p.report_size() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    // SAFETY: non-null, fixed-size input; caller guarantees memory readability.
    let state = match InputState::decode(unsafe { core::slice::from_raw_parts(input, length) }) {
        Ok(s) => s,
        Err(_) => return STATUS_INVALID_PACKET,
    };
    let Some(report) = profiles::encode(p, &state) else {
        return STATUS_INVALID_PACKET;
    };
    // SAFETY: sufficient destination capacity and local source cannot overlap.
    unsafe {
        core::ptr::copy_nonoverlapping(report.as_ptr(), output, p.report_size());
    }
    STATUS_OK
}

/// # Safety
/// Output is writable for capacity bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_neutral_report_for_profile(
    raw: u32,
    output: *mut u8,
    capacity: usize,
) -> u32 {
    let packet = InputState::NEUTRAL.encode();
    // SAFETY: local packet is readable; output validity is the caller's contract.
    unsafe {
        scx_vpad_encode_report_for_profile(raw, packet.as_ptr(), packet.len(), output, capacity)
    }
}

/// # Safety
/// Output is writable for capacity bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_get_info_for_profile(
    raw: u32,
    output: *mut u8,
    capacity: usize,
) -> u32 {
    if output.is_null() {
        return STATUS_INVALID_POINTER;
    }
    let p = match profile(raw) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let info = DeviceInfo::for_profile(p).encode();
    if capacity < info.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    // SAFETY: destination capacity is checked; the caller owns valid storage.
    unsafe {
        core::ptr::copy_nonoverlapping(info.as_ptr(), output, info.len());
    }
    STATUS_OK
}

/// # Safety
/// All three destinations point to writable u16 storage, optionally unaligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_get_hid_attributes(
    raw: u32,
    vendor: *mut u16,
    product: *mut u16,
    version: *mut u16,
) -> u32 {
    if vendor.is_null() || product.is_null() || version.is_null() {
        return STATUS_INVALID_POINTER;
    }
    let Some((v, p, r)) = profile(raw).ok().and_then(profiles::attributes) else {
        return UNSUPPORTED;
    };
    // SAFETY: all output addresses are non-null, writable, and caller-owned.
    unsafe {
        core::ptr::write_unaligned(vendor, v);
        core::ptr::write_unaligned(product, p);
        core::ptr::write_unaligned(version, r);
    }
    STATUS_OK
}

/// Returns the complete feature report INCLUDING report ID. Unknown reports fail.
/// # Safety
/// Output is writable for capacity; output_length points to a writable usize.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_feature_report_for_profile(
    raw: u32,
    report_id: u8,
    instance_id: u64,
    output: *mut u8,
    capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if output.is_null() || output_length.is_null() {
        return STATUS_INVALID_POINTER;
    }
    // SAFETY: caller owns a valid output length slot.
    unsafe {
        core::ptr::write_unaligned(output_length, 0);
    }
    let Some((report, len)) = profile(raw)
        .ok()
        .and_then(|p| profiles::feature(p, report_id, instance_id))
    else {
        return UNSUPPORTED;
    };
    if capacity < len {
        return STATUS_BUFFER_TOO_SMALL;
    }
    // SAFETY: sufficient output storage, non-overlapping local report, valid length slot.
    unsafe {
        core::ptr::copy_nonoverlapping(report.as_ptr(), output, len);
        core::ptr::write_unaligned(output_length, len);
    }
    STATUS_OK
}

/// Validates the two Sony radio-configuration feature writes seen during native
/// game initialization. A virtual USB pad has no Bluetooth radio to configure:
/// these are acknowledged without changing input, pairing identity or feedback.
/// All other feature writes (especially the private producer IDs) stay denied.
#[unsafe(no_mangle)]
pub extern "C" fn scx_vpad_validate_configuration_feature(
    raw: u32,
    report_id: u8,
    length: usize,
) -> u32 {
    let expected = match (raw, report_id) {
        (3, 0x14) => 17,
        (4, 0x08) => 48,
        _ => return UNSUPPORTED,
    };
    if length != expected {
        return STATUS_INVALID_LENGTH;
    }
    STATUS_OK
}

/// # Safety
/// Input is readable for length and output writable for capacity; overlap allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_parse_output_for_profile(
    raw: u32,
    input: *const u8,
    length: usize,
    output: *mut u8,
    capacity: usize,
) -> u32 {
    if input.is_null() || output.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if capacity < scx_vpad_protocol::FEEDBACK_PACKET_SIZE {
        return STATUS_BUFFER_TOO_SMALL;
    }
    let p = match profile(raw) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !matches!(p, Profile::DualShock4 | Profile::DualSense) {
        return UNSUPPORTED;
    }
    if !matches!(
        (p, length),
        (Profile::DualShock4, 32) | (Profile::DualSense, 48 | 63)
    ) {
        return STATUS_INVALID_LENGTH;
    }
    // SAFETY: valid fixed input length, readable memory guaranteed by C caller.
    let feedback =
        match profiles::parse_output(p, unsafe { core::slice::from_raw_parts(input, length) }) {
            Ok(f) => f,
            Err(_) => return STATUS_INVALID_PACKET,
        }
        .encode();
    // SAFETY: checked output capacity; local encoded value cannot overlap destination.
    unsafe {
        core::ptr::copy_nonoverlapping(feedback.as_ptr(), output, feedback.len());
    }
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use scx_vpad_protocol::{CAP_EXPERIMENTAL, CAP_XINPUT};
    #[test]
    fn sony_configuration_writes_are_separate_from_producer_authorization() {
        for (raw, id, length) in [(3, 0x14, 17), (4, 0x08, 48)] {
            assert_eq!(
                scx_vpad_validate_configuration_feature(raw, id, length),
                STATUS_OK
            );
            for invalid in [0, 1, length - 1, length + 1, 64, 97, usize::MAX] {
                assert_eq!(
                    scx_vpad_validate_configuration_feature(raw, id, invalid),
                    STATUS_INVALID_LENGTH
                );
            }
            for other in 0..=u8::MAX {
                if other != id {
                    assert_eq!(
                        scx_vpad_validate_configuration_feature(raw, other, length),
                        UNSUPPORTED
                    );
                }
            }
        }
        for raw in [0, 1, 2, 5, u32::MAX] {
            assert_eq!(
                scx_vpad_validate_configuration_feature(raw, 0x14, 17),
                UNSUPPORTED
            );
            assert_eq!(
                scx_vpad_validate_configuration_feature(raw, 0x08, 48),
                UNSUPPORTED
            );
        }
    }
    #[test]
    fn xbox_info_cannot_be_mistaken_for_a_hid_profile() {
        let mut info = [0; 32];
        let mut length = usize::MAX;
        let mut report = [0xcc; 64];
        // SAFETY: all pointers name correctly sized local test storage.
        unsafe {
            assert_eq!(
                scx_vpad_get_info_for_profile(2, info.as_mut_ptr(), info.len()),
                0
            );
            let parsed = DeviceInfo::decode(&info).unwrap();
            assert_eq!(parsed.profile, Profile::Xbox360);
            assert_eq!(parsed.capabilities, CAP_XINPUT | CAP_EXPERIMENTAL);
            assert_eq!(scx_vpad_profile_report_size(2), 0);
            assert!(scx_vpad_report_descriptor_for_profile(2, &mut length).is_null());
            assert_eq!(length, 0);
            assert_eq!(
                scx_vpad_neutral_report_for_profile(2, report.as_mut_ptr(), report.len()),
                UNSUPPORTED
            );
            assert_eq!(report, [0xcc; 64]);
        }
    }
    #[test]
    fn profile_ffi_preserves_boundaries_and_refuses_unknown_modes() {
        let mut buffer = [0xcd; 66];
        let mut len = 0;
        // SAFETY: every destination points to valid local storage of stated size.
        unsafe {
            assert_eq!(
                scx_vpad_neutral_report_for_profile(4, buffer.as_mut_ptr().add(1), 64),
                0
            );
            assert_eq!(buffer[0], 0xcd);
            assert_eq!(buffer[65], 0xcd);
            assert_eq!(
                scx_vpad_neutral_report_for_profile(2, buffer.as_mut_ptr(), 64),
                UNSUPPORTED
            );
            assert_eq!(
                scx_vpad_feature_report_for_profile(4, 0xff, 1, buffer.as_mut_ptr(), 64, &mut len),
                UNSUPPORTED
            );
            assert_eq!(len, 0);
            assert_eq!(
                scx_vpad_feature_report_for_profile(4, 5, 1, buffer.as_mut_ptr(), 64, &mut len),
                0
            );
            assert_eq!(len, 41);
        }
    }
}
