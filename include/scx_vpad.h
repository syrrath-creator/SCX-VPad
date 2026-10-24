/* Original SCX Virtual Pad ABI. See ../LICENSE for distribution terms. */
#ifndef SCX_VPAD_H
#define SCX_VPAD_H
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SCX_VPAD_ABI_VERSION 1u
#define SCX_VPAD_DEVICE_INFO_VERSION 2u
#define SCX_VPAD_AUTH_CAPABILITY_SIZE 32u
#define SCX_VPAD_HID_SUBMIT_PAYLOAD_SIZE 96u
#define SCX_VPAD_HID_FEATURE_REPORT_SIZE 97u
#define SCX_VPAD_CAP_AUTHENTICATED_INPUT 16u
#define SCX_VPAD_CAP_RAW_JOYSTICK 32u
#define SCX_VPAD_CAP_ARCADE_STICK 64u
#define SCX_VPAD_MAX_DEVICES 16u
#define SCX_VPAD_INPUT_PACKET_SIZE 64u
#define SCX_VPAD_INPUT_REPORT_SIZE 18u
#define SCX_VPAD_INFO_PACKET_SIZE 32u
#define SCX_VPAD_FEEDBACK_PACKET_SIZE 32u
#define SCX_VPAD_FEEDBACK_FEATURE_ID 0x7du
#define SCX_VPAD_MAX_REPORT_SIZE 64u
#define SCX_VPAD_GAMEPAD_REPORT_ID 1u
#define SCX_VPAD_SUBMIT_FEATURE_ID 0x7au
#define SCX_VPAD_INFO_FEATURE_ID 0x7fu
#define SCX_VPAD_CONTROL_USAGE_PAGE 0xff53u
#define SCX_VPAD_CONTROL_USAGE 1u

typedef enum SCX_VPAD_PROFILE {
    SCX_PROFILE_GENERIC_HID = 1,
    SCX_PROFILE_XBOX360 = 2,
    SCX_PROFILE_DUALSHOCK4 = 3,
    SCX_PROFILE_DUALSENSE = 4,
    /* 5 remains reserved for the unsupported Steam persona. */
    SCX_PROFILE_WHEEL = 6,
    SCX_PROFILE_FLIGHT_STICK = 7,
    SCX_PROFILE_ARCADE_STICK = 8
} SCX_VPAD_PROFILE;
#define SCX_VPAD_SUBMIT_CAPABILITY_OFFSET 1u
#define SCX_VPAD_SUBMIT_INPUT_OFFSET (1u + SCX_VPAD_AUTH_CAPABILITY_SIZE)
#define SCX_VPAD_INPUT_SEQUENCE_OFFSET 4u
#define SCX_VPAD_FEEDBACK_SEQUENCE_OFFSET 4u
#define SCX_VPAD_FEEDBACK_FIELDS_OFFSET 8u
#define SCX_VPAD_FEEDBACK_PLAYER_VALID 4u
#define SCX_VPAD_HID_QUEUE_CAPACITY 64u
#define SCX_VPAD_XUSB_STATE_REPORT_SIZE 29u
#define SCX_VPAD_XUSB_CONTROLS_OFFSET 11u
#define SCX_VPAD_XUSB_CONTROLS_SIZE 12u

/* Private in-process storage ABI (not a packed/wire struct). Rust owns ALL
 * fields; C only allocates/aligns the storage. Initialized once per new device,
 * never shared with untrusted producers; serialized by the device's WDF lock. */
typedef struct SCX_VPAD_HID_STATE {
    uint32_t private_metadata[6];
    uint8_t private_reports[SCX_VPAD_HID_QUEUE_CAPACITY][SCX_VPAD_MAX_REPORT_SIZE];
    uint8_t private_current[SCX_VPAD_MAX_REPORT_SIZE];
    uint8_t private_neutral[SCX_VPAD_MAX_REPORT_SIZE];
} SCX_VPAD_HID_STATE;

/* All packets are explicitly little-endian; never cast a buffer to a struct.
 * Input64: u16 version@0, u16 size@2, u32 sequence@4, u32 buttons@8,
 * i16 leftX/Y,rightX/Y@12/14/16/18, u16 triggers@20/22, u8 hat@24,
 * u8 flags@25, reserved zero u16@26, i16 gyro[3]@28 (1/16 deg/sec),
 * i16 accel[3]@34 (milli-g), two six-byte touch records@40/46
 * {u16 x,y;u8 id,flags}, u64 timestamp_us@52, reserved zero u32@60.
 * Hat: 0=N,1=NE,...7=NW,8=neutral. Flags: gyro valid=1, accel=2,
 * touch=4. Touch flags: active=1. Unknown/reserved bits are rejected.
 * Sensors/touch are reserved transport fields, NOT advertised gamepad
 * capabilities until a compatible output profile is implemented.
 *
 * Info32: 'SCVP'@0, u16 metadata version2@4, u16 size@6, u32 profile@8,
 * u32 capabilities@12 (generic=1|16), u16 submit size@16 (HID96, Xbox64),
 * u16 input report size@18, u16 max devices@20, reserved u16@22,
 * u32 implementation version@24 (=0x00000100), reserved u32@28.
 *
 * The feature report ID is NOT included in these packet lengths.
 * HidD_SetFeature uses exactly [0x7a, capability32, Input64]. Native code must
 * authenticate the full capability before decoding input or mutating any state.
 * Legacy feature0x7e is removed and must always fail. Never return a capability
 * from GetFeature or diagnostics. HidD_GetFeature returns [0x7f, Info32].
 * HID callers allocate the collection's maximum FeatureReportByteLength (97)
 * for either request; information bytes after index32 are zero padding.
 */
typedef enum SCX_VPAD_STATUS {
    SCX_VPAD_OK = 0,
    SCX_VPAD_INVALID_POINTER = 1,
    SCX_VPAD_INVALID_LENGTH = 2,
    SCX_VPAD_INVALID_PACKET = 3,
    SCX_VPAD_BUFFER_TOO_SMALL = 4,
    SCX_VPAD_UNSUPPORTED = 5,
    SCX_VPAD_QUEUE_FULL = 6
} SCX_VPAD_STATUS;

/* No ownership transfer. Caller owns readable/writable buffers and guarantees
 * validity for the given lengths. Null and unsupported lengths are rejected.
 * Input/output may overlap: the input is decoded fully before output is written.
 * No function allocates or intentionally panics; driver builds use panic=abort.
 */
uint32_t scx_vpad_validate_input(const uint8_t *input, size_t length);
uint32_t scx_vpad_input_is_neutral(const uint8_t *input, size_t length, uint8_t *neutral);
uint32_t scx_vpad_encode_report(const uint8_t *input, size_t length,
                              uint8_t *output, size_t capacity);
uint32_t scx_vpad_neutral_report(uint8_t *output, size_t capacity);
uint32_t scx_vpad_get_info(uint8_t *output, size_t capacity);
const uint8_t *scx_vpad_report_descriptor(size_t *output_length);

/* State and packet/output buffers must not overlap. Init must succeed before
 * other state calls. front/current pointers remain valid only until mutation;
 * retain the device lock while copying, then consume only on successful copy. */
uint32_t scx_vpad_hid_init(SCX_VPAD_HID_STATE *state, size_t size, uint32_t profile);
uint32_t scx_vpad_hid_submit(SCX_VPAD_HID_STATE *state, const uint8_t *input,
    size_t length, uint8_t *neutral);
const uint8_t *scx_vpad_hid_front(const SCX_VPAD_HID_STATE *state);
const uint8_t *scx_vpad_hid_current(const SCX_VPAD_HID_STATE *state);
void scx_vpad_hid_consume(SCX_VPAD_HID_STATE *state);
void scx_vpad_hid_neutralize(SCX_VPAD_HID_STATE *state);
void scx_vpad_hid_clear(SCX_VPAD_HID_STATE *state);
uint8_t scx_vpad_sequence_after(uint32_t sequence, uint32_t previous);
uint32_t scx_vpad_feedback_init(uint32_t profile, uint8_t *output, size_t capacity);
uint32_t scx_vpad_feedback_merge(uint8_t *current, size_t length,
    const uint8_t *update, size_t update_length, uint8_t *changed);

/* Immutable profile IDs: 1=generic HID, 2=experimental Xbox XUSB function device,
 * 3=experimental DualShock4 USB, 4=experimental DualSense USB. 5=Steam unsupported.
 * 6=wheel, 7=flight stick, 8=arcade stick: experimental generic HID classes,
 * sharing Input64/18-byte input and the authenticated generic vendor collection.
 * No force feedback, adaptive triggers, or keyboard/mouse device injection.
 * Profile-specific codecs are implemented; game/driver compatibility must be
 * verified separately. CAP_EXPERIMENTAL (0x80000000) is set for profiles 2/3/4/6/7/8.
 * Sony HID input report size is 64 rather than generic18. Xbox HID report size
 * is zero, descriptor/attributes/encode/feature are unsupported; its INFO has
 * CAP_XINPUT (8), with no CAP_GENERIC_GAMEPAD, and uses the separate XUSB API.
 */
size_t scx_vpad_profile_report_size(uint32_t profile);
const uint8_t *scx_vpad_report_descriptor_for_profile(uint32_t profile, size_t *output_length);
uint32_t scx_vpad_encode_report_for_profile(uint32_t profile, const uint8_t *input,
    size_t length, uint8_t *output, size_t capacity);
uint32_t scx_vpad_neutral_report_for_profile(uint32_t profile, uint8_t *output, size_t capacity);
uint32_t scx_vpad_get_info_for_profile(uint32_t profile, uint8_t *output, size_t capacity);
uint32_t scx_vpad_get_hid_attributes(uint32_t profile, uint16_t *vendor,
    uint16_t *product, uint16_t *version);
/* Feature output includes report ID; no unknown feature silently succeeds. */
uint32_t scx_vpad_feature_report_for_profile(uint32_t profile, uint8_t report_id,
    uint64_t instance_id, uint8_t *output, size_t capacity, size_t *output_length);
/* Sony radio configuration only: DS4 0x14/17 bytes, DS5 0x08/48 bytes (ID included).
 * Pure validation, no state mutation or producer authorization. */
uint32_t scx_vpad_validate_configuration_feature(uint32_t profile, uint8_t report_id, size_t length);
/* Output report includes its ID. Feedback32 layout:
 * u16 version@0,size@2; u32 sequence@4 (driver assigns), valid-fields@8;
 * u16 leftmotor@12,rightmotor@14; u8 RGB@16..18,player@19,mic@20,
 * blink-on/off@21/22 (10ms units),zero@23; u32 profile@24,zero@28.
 * Valid-fields: rumble1,RGB2,player4,mic8,blink16. Merge only valid fields
 * into last feedback and increment sequence; an RGB update must not stop rumble.
 */
uint32_t scx_vpad_parse_output_for_profile(uint32_t profile, const uint8_t *input,
    size_t length, uint8_t *feedback, size_t capacity);

/* Experimental XUSB function-device ABI; must not be placed behind HIDCLASS.
 * Packet number belongs to device context and advances only when the encoded
 * controls change. player_index is 0..3 (standard XInput's global limit).
 * query input packet may be null/zero except for GET_STATE, which requires64.
 * Request may be null only at request_length0. Unsupported commands return5.
 */
uint32_t scx_vpad_xusb_query(uint32_t code, const uint8_t *request, size_t request_length,
    const uint8_t *input, size_t input_length, uint32_t packet_number,
    uint8_t player_index, uint8_t led_state, uint8_t *output, size_t capacity,
    size_t *output_length);
uint32_t scx_vpad_xusb_decode_output(const uint8_t *request, size_t length,
    uint8_t *feedback, size_t capacity);
uint32_t scx_vpad_xusb_state_report(const uint8_t *input, size_t length,
    uint32_t packet_number, uint8_t resumed, uint8_t *output, size_t capacity);

#ifdef __cplusplus
}
#endif
#endif
