/* Original SCX C-to-Rust ABI smoke test; no driver or device is installed. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "scx_vpad.h"

#define CHECK(c) do { if (!(c)) { fprintf(stderr, "ABI check failed at line %d: %s\n", __LINE__, #c); return 1; } } while (0)

_Static_assert(sizeof(SCX_VPAD_HID_STATE) == 4248, "Rust state storage size");
_Static_assert(_Alignof(SCX_VPAD_HID_STATE) == 4, "Rust state alignment");
_Static_assert(offsetof(SCX_VPAD_HID_STATE, private_current) == 4120, "Rust current offset");

/* Exercise exactly the FFI functions used by WDF, including unsuccessful reads,
 * timeout/replay handling, four independent devices and output field merging. */
static int runtime_check(void)
{
    struct { uint32_t before; SCX_VPAD_HID_STATE state; uint32_t after; } devices[4];
    uint8_t packet[64] = {1,0,64,0}, expected[64], neutral = 0x5a;
    uint8_t feedback[32], update[32], before[32], changed;
    uint32_t profile, i, device, sequence;
    for (profile = SCX_PROFILE_GENERIC_HID; profile <= SCX_PROFILE_ARCADE_STICK; ++profile) {
        if (profile == SCX_PROFILE_XBOX360 || profile == 5) continue;
        memset(devices, 0xa5, sizeof(devices));
        for (device = 0; device < 4; ++device) {
            SCX_VPAD_HID_STATE *q = &devices[device].state;
            CHECK(scx_vpad_hid_init(q, sizeof(*q) - 1, profile) == SCX_VPAD_INVALID_LENGTH);
            CHECK(scx_vpad_hid_init(q, sizeof(*q), profile) == SCX_VPAD_OK);
            CHECK(scx_vpad_neutral_report_for_profile(profile, expected, sizeof(expected)) == 0);
            CHECK(memcmp(scx_vpad_hid_front(q), expected, scx_vpad_profile_report_size(profile)) == 0);
            scx_vpad_hid_consume(q);
            memset(packet, 0, sizeof(packet)); packet[0] = 1; packet[2] = 64; packet[24] = 8;
            for (i = 0; i < 64; ++i) {
                memcpy(packet + SCX_VPAD_INPUT_SEQUENCE_OFFSET, &i, sizeof(i));
                packet[8] = (uint8_t)(1u << (device + i % 2));
                CHECK(scx_vpad_hid_submit(q, packet, sizeof(packet), &neutral) == 0 && neutral == 0);
            }
            i = 64; memcpy(packet + SCX_VPAD_INPUT_SEQUENCE_OFFSET, &i, sizeof(i)); packet[8] = 0;
            CHECK(scx_vpad_hid_submit(q, packet, sizeof(packet), &neutral) == SCX_VPAD_QUEUE_FULL);
            CHECK(neutral == 0); /* failed submit does not change its output */
            memcpy(before, scx_vpad_hid_front(q), 18);
            CHECK(memcmp(before, scx_vpad_hid_front(q), 18) == 0); /* failed reader retains transition */
            scx_vpad_hid_consume(q);
            CHECK(scx_vpad_hid_submit(q, packet, sizeof(packet), &neutral) == 0 && neutral == 1);
            CHECK(scx_vpad_hid_submit(q, packet, sizeof(packet), &neutral) == SCX_VPAD_INVALID_PACKET);
            scx_vpad_hid_neutralize(q);
            CHECK(memcmp(scx_vpad_hid_front(q), expected, scx_vpad_profile_report_size(profile)) == 0);
            CHECK(scx_vpad_hid_submit(q, packet, sizeof(packet), &neutral) == SCX_VPAD_INVALID_PACKET);
            scx_vpad_hid_consume(q); CHECK(scx_vpad_hid_front(q) == NULL);
            CHECK(scx_vpad_hid_init(q, sizeof(*q), profile) == 0);
            sequence = UINT32_MAX; memcpy(packet + 4, &sequence, 4);
            CHECK(scx_vpad_hid_submit(q, packet, sizeof(packet), &neutral) == 0);
            sequence = 0; memcpy(packet + 4, &sequence, 4);
            CHECK(scx_vpad_hid_submit(q, packet, sizeof(packet), &neutral) == 0);
            CHECK(devices[device].before == 0xa5a5a5a5u && devices[device].after == 0xa5a5a5a5u);
        }
    }
    CHECK(scx_vpad_sequence_after(0, UINT32_MAX) == 1);
    CHECK(scx_vpad_sequence_after(0x80000000u, 0) == 0);
    for (profile = SCX_PROFILE_XBOX360; profile <= SCX_PROFILE_DUALSENSE; ++profile) {
        CHECK(scx_vpad_feedback_init(profile, feedback, sizeof(feedback)) == 0);
        CHECK(scx_vpad_feedback_init(profile, update, sizeof(update)) == 0);
        update[8] = 1; update[12] = 123; update[14] = 45;
        CHECK(scx_vpad_feedback_merge(feedback, 32, update, 32, &changed) == 0 && changed == 1);
        CHECK(feedback[4] == 1 && feedback[12] == 123 && feedback[14] == 45);
        CHECK(scx_vpad_feedback_merge(feedback, 32, update, 32, &changed) == 0 && changed == 0);
        CHECK(feedback[4] == 1);
        update[8] = 30; update[16] = 200; update[19] = 2; update[20] = 1; update[21] = 9; update[22] = 8;
        CHECK(scx_vpad_feedback_merge(feedback, 32, update, 32, &changed) == 0 && changed == 1);
        CHECK(feedback[4] == 2 && feedback[12] == 123 && feedback[14] == 45);
        CHECK(feedback[16] == 200 && feedback[19] == 2 && feedback[20] == 1 && feedback[21] == 9 && feedback[22] == 8);
        memcpy(before, feedback, sizeof(before)); update[23] = 1; changed = 0x5a;
        CHECK(scx_vpad_feedback_merge(feedback, 32, update, 32, &changed) == SCX_VPAD_INVALID_PACKET);
        CHECK(memcmp(before, feedback, 32) == 0 && changed == 0x5a);
        update[23] = 0; update[24] = (uint8_t)(profile == 2 ? 3 : 2);
        CHECK(scx_vpad_feedback_merge(feedback, 32, update, 32, &changed) == SCX_VPAD_INVALID_PACKET);
        CHECK(memcmp(before, feedback, 32) == 0);
        update[24] = (uint8_t)profile; update[8] = 1; update[12] = update[14] = 0;
        CHECK(scx_vpad_feedback_merge(feedback, 32, update, 32, &changed) == 0 && changed == 1);
        CHECK(feedback[4] == 3 && feedback[12] == 0 && feedback[14] == 0 && feedback[16] == 200);
    }
    return 0;
}

int main(void)
{
    uint8_t frame[64] = {1,0,64,0};
    uint8_t output[80];
    uint8_t neutral[18];
    uint8_t info[32];
    const uint8_t *descriptor;
    size_t descriptorLength;
    unsigned reportId = 0, reportSize = 0, reportCount = 0;
    unsigned inputBits[256] = {0}, featureBits[256] = {0};
    unsigned depth = 0, collections = 0;
    size_t offset = 0;
    frame[24] = 8;
    CHECK(scx_vpad_validate_input(frame, sizeof(frame)) == 0);
    CHECK(scx_vpad_validate_input(NULL, sizeof(frame)) != 0);
    CHECK(scx_vpad_validate_input(frame, sizeof(frame) - 1) != 0);
    memset(output, 0x5a, sizeof(output));
    CHECK(scx_vpad_encode_report(frame, sizeof(frame), output + 1, 18) == 0);
    CHECK(output[0] == 0x5a && output[19] == 0x5a);
    CHECK(scx_vpad_neutral_report(neutral, sizeof(neutral)) == 0);
    CHECK(memcmp(output + 1, neutral, sizeof(neutral)) == 0);
    CHECK(neutral[0] == 1 && neutral[5] == 8);
    frame[8] = 0xa5;
    frame[9] = 0x5a;
    CHECK(scx_vpad_encode_report(frame, sizeof(frame), output + 1, 18) == 0);
    CHECK(output[2] == 0xa5 && output[3] == 0x5a);
    frame[60] = 1;
    CHECK(scx_vpad_validate_input(frame, sizeof(frame)) != 0);
    frame[60] = 0;
    memset(output, 0x5a, sizeof(output));
    CHECK(scx_vpad_encode_report(frame, sizeof(frame), output + 1, 17) != 0);
    for (offset = 0; offset < sizeof(output); ++offset) CHECK(output[offset] == 0x5a);
    CHECK(scx_vpad_get_info(info, sizeof(info)) == 0);
    CHECK(memcmp(info, "SCVP", 4) == 0 && info[4] == 2 && info[6] == 32);

    /* Parse HID short items independently from Rust's encoder. This catches a
     * mismatched descriptor/report ABI before Windows sees the package. */
    descriptor = scx_vpad_report_descriptor(&descriptorLength);
    CHECK(descriptor != NULL && descriptorLength != 0);
    offset = 0;
    while (offset < descriptorLength) {
        unsigned prefix = descriptor[offset++];
        unsigned size = prefix & 3u;
        unsigned type = (prefix >> 2) & 3u;
        unsigned tag = prefix >> 4;
        unsigned value = 0, index;
        CHECK(prefix != 0xfe);
        if (size == 3) size = 4;
        CHECK(offset + size <= descriptorLength);
        for (index = 0; index < size; ++index) value |= (unsigned)descriptor[offset + index] << (8u * index);
        offset += size;
        if (type == 1) {
            if (tag == 7) reportSize = value;
            if (tag == 8) { CHECK(value < 256); reportId = value; }
            if (tag == 9) reportCount = value;
        } else if (type == 0) {
            if (tag == 10) { if (depth == 0) ++collections; ++depth; }
            if (tag == 12) { CHECK(depth > 0); --depth; }
            if (tag == 8) inputBits[reportId] += reportSize * reportCount;
            if (tag == 11) featureBits[reportId] += reportSize * reportCount;
        }
    }
    CHECK(depth == 0 && collections == 2);
    CHECK(inputBits[1] == 17u * 8u);
    CHECK(featureBits[0x7e] == 0);
    CHECK(featureBits[0x78] == 0 && featureBits[0x79] == 0 && featureBits[0x7c] == 0 && featureBits[0x7d] == 0);
    CHECK(featureBits[0x7a] == 96u * 8u);
    CHECK(featureBits[0x7f] == 32u * 8u);
    {
        uint32_t profile;
        uint16_t vendor, product, version;
        for (profile = SCX_PROFILE_WHEEL; profile <= SCX_PROFILE_ARCADE_STICK; ++profile) {
            uint8_t expected[18];
            const unsigned usage = profile == SCX_PROFILE_ARCADE_STICK ? 5u : 4u;
            descriptor = scx_vpad_report_descriptor_for_profile(profile, &descriptorLength);
            CHECK(descriptor != NULL && descriptorLength >= 6);
            CHECK(descriptor[0] == 5 && descriptor[1] == 1 && descriptor[2] == 9 && descriptor[3] == usage);
            CHECK(scx_vpad_profile_report_size(profile) == 18);
            CHECK(scx_vpad_get_hid_attributes(profile, &vendor, &product, &version) == 0);
            CHECK(vendor == 0 && product == 0x5340u + profile && version == 0x100);
            CHECK(scx_vpad_get_info_for_profile(profile, info, sizeof(info)) == 0);
            CHECK(info[8] == profile && info[18] == 18 && (info[15] & 0x80) != 0);
            memset(output, 0x5a, sizeof(output));
            CHECK(scx_vpad_encode_report_for_profile(profile, frame, sizeof(frame), output + 1, 18) == 0);
            CHECK(output[0] == 0x5a && output[19] == 0x5a);
            CHECK(scx_vpad_encode_report(frame, sizeof(frame), expected, sizeof(expected)) == 0);
            CHECK(memcmp(output + 1, expected, sizeof(expected)) == 0);
            memset(output, 0x5a, sizeof(output));
            CHECK(scx_vpad_encode_report_for_profile(profile, frame, sizeof(frame), output + 1, 17) != 0);
            for (offset = 0; offset < sizeof(output); ++offset) CHECK(output[offset] == 0x5a);
            CHECK(scx_vpad_validate_configuration_feature(profile, 0x14, 17) == SCX_VPAD_UNSUPPORTED);
            CHECK(scx_vpad_parse_output_for_profile(profile, frame, sizeof(frame), output, sizeof(output)) == SCX_VPAD_UNSUPPORTED);
        }
        CHECK(scx_vpad_profile_report_size(5) == 0);
        CHECK(scx_vpad_report_descriptor_for_profile(5, &descriptorLength) == NULL);
    }
    {
        uint32_t profile;
        uint8_t feedback[32], feature[64], raw[63] = {2,3};
        uint8_t isNeutral;
        size_t featureLength;
        uint16_t vendor, product, version;
        memset(frame, 0, sizeof(frame));
        frame[0] = 1; frame[2] = 64; frame[24] = 8; frame[4] = 12;
        CHECK(scx_vpad_input_is_neutral(frame, sizeof(frame), &isNeutral) == 0 && isNeutral == 1);
        frame[8] = 1;
        CHECK(scx_vpad_input_is_neutral(frame, sizeof(frame), &isNeutral) == 0 && isNeutral == 0);
        frame[8] = 0;
        for (profile = 3; profile <= 4; ++profile) {
            memset(output, 0x5a, sizeof(output));
            CHECK(scx_vpad_profile_report_size(profile) == 64);
            CHECK(scx_vpad_encode_report_for_profile(profile, frame, sizeof(frame), output + 1, 64) == 0);
            CHECK(output[0] == 0x5a && output[65] == 0x5a);
            CHECK(output[1] == 1 && output[2] == 128 && output[3] == 128);
            CHECK(scx_vpad_get_hid_attributes(profile, &vendor, &product, &version) == 0);
            CHECK(vendor == 0x054c && product == (profile == 3 ? 0x09cc : 0x0ce6));
            CHECK(scx_vpad_get_info_for_profile(profile, info, sizeof(info)) == 0);
            CHECK(info[8] == profile && (info[15] & 0x80) != 0 && info[18] == 64);
            CHECK(scx_vpad_feature_report_for_profile(profile, profile == 3 ? 0x12 : 9,
                       123, feature, sizeof(feature), &featureLength) == 0);
            CHECK(feature[1] == 123 && feature[6] == 2);
            CHECK(scx_vpad_feature_report_for_profile(profile, 0xff, 123,
                       feature, sizeof(feature), &featureLength) != 0);
            CHECK(scx_vpad_validate_configuration_feature(profile, profile == 3 ? 0x14 : 8,
                       profile == 3 ? 17 : 48) == SCX_VPAD_OK);
            CHECK(scx_vpad_validate_configuration_feature(profile, profile == 3 ? 0x14 : 8,
                       97) == SCX_VPAD_INVALID_LENGTH);
            CHECK(scx_vpad_validate_configuration_feature(profile, 0x7a, 97) == SCX_VPAD_UNSUPPORTED);
        }
        CHECK(scx_vpad_profile_report_size(2) == 0);
        raw[3] = 10; raw[4] = 20;
        CHECK(scx_vpad_parse_output_for_profile(4, raw, sizeof(raw), feedback, sizeof(feedback)) == 0);
        CHECK(feedback[8] == 1 && feedback[12] == 20 && feedback[13] == 20);
        raw[1] = 0x04; /* No trigger engine: accept but do not invent feedback. */
        CHECK(scx_vpad_parse_output_for_profile(4, raw, sizeof(raw), feedback, sizeof(feedback)) == 0);
        CHECK(feedback[8] == 0);
        raw[1] = 0x0d; raw[2] = 0x17; /* Mixed native-game initialization. */
        CHECK(scx_vpad_parse_output_for_profile(4, raw, 48, feedback, sizeof(feedback)) == 0);
        CHECK(feedback[8] == 15 && feedback[12] == 20 && feedback[14] == 10);
    }
    {
        uint8_t request[3] = {3,1,0};
        uint8_t setState[5] = {0,7,255,32,2};
        uint8_t feedback[32];
        size_t length = 0;
        memset(frame, 0, sizeof(frame));
        frame[0] = 1; frame[2] = 64; frame[24] = 8;
        frame[8] = 1; /* Canonical south/A. */
        memset(output, 0x5a, sizeof(output));
        CHECK(scx_vpad_get_info_for_profile(2, info, sizeof(info)) == 0);
        CHECK(info[8] == 2 && (info[15] & 0x80) != 0 && info[18] == 0);
        CHECK(scx_vpad_xusb_query(0x80006000u, NULL, 0, NULL, 0, 0, 0, 6,
                                 output + 1, 36, &length) == 0);
        CHECK(length == 12 && output[0] == 0x5a && output[13] == 0x5a);
        CHECK(scx_vpad_xusb_query(0x8000e00cu, request, 3, frame, 64, 42, 0, 6,
                                 output + 1, 36, &length) == 0);
        CHECK(length == 29 && output[6] == 42 && output[13] == 0x10);
        CHECK(output[0] == 0x5a && output[30] == 0x5a);
        CHECK(scx_vpad_xusb_decode_output(setState, 5, feedback, sizeof(feedback)) == 0);
        CHECK(feedback[8] == 1 && feedback[12] == 255 && feedback[13] == 255);
        CHECK(feedback[14] == 32 && feedback[15] == 32 && feedback[24] == 2);
        request[2] = 1;
        CHECK(scx_vpad_xusb_query(0x8000e00cu, request, 3, frame, 64, 42, 0, 6,
                                 output, sizeof(output), &length) != 0 && length == 0);
    }
    CHECK(runtime_check() == 0);
    puts("SCX native/Rust ABI, actual HID queue, feedback merge, descriptors and XUSB checks: PASS");
    return 0;
}
