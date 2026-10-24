//! Parse descriptors independently of encoders: a wrong usage or bit offset can
//! produce plausible bytes while Windows binds the wrong physical control.
use super::*;

#[derive(Debug)]
struct Field {
    usage: (u16, u16),
    offset: usize,
    width: usize,
    min: i32,
    max: i32,
    flags: u32,
}
struct Layout {
    fields: Vec<Field>,
    collections: Vec<(u16, u16)>,
    input: [usize; 256],
    feature: [usize; 256],
    output: [usize; 256],
}
fn parse(d: &[u8]) -> Layout {
    let mut result = Layout {
        fields: Vec::new(),
        collections: Vec::new(),
        input: [0; 256],
        feature: [0; 256],
        output: [0; 256],
    };
    let (mut pos, mut page, mut id, mut size, mut count, mut depth) = (0, 0u16, 0, 0, 0, 0);
    let (mut min, mut max) = (0, 0);
    let (mut usages, mut range) = (Vec::new(), None);
    while pos < d.len() {
        let tag = d[pos];
        assert_ne!(tag, 0xfe, "this descriptor has no HID long items");
        let n = match tag & 3 {
            3 => 4,
            n => usize::from(n),
        };
        let bytes = &d[pos + 1..pos + 1 + n];
        let unsigned = bytes
            .iter()
            .enumerate()
            .fold(0u32, |v, (i, b)| v | u32::from(*b) << (i * 8));
        let signed = if n == 0 {
            0
        } else {
            ((unsigned << (32 - n * 8)) as i32) >> (32 - n * 8)
        };
        match tag & 0xfc {
            0x04 => page = unsigned as u16,
            0x14 => min = signed,
            0x24 => max = if min < 0 { signed } else { unsigned as i32 },
            0x74 => size = unsigned as usize,
            0x94 => count = unsigned as usize,
            0x84 => id = unsigned as usize,
            0x08 => usages.push(if n == 4 {
                ((unsigned >> 16) as u16, unsigned as u16)
            } else {
                (page, unsigned as u16)
            }),
            0x18 => range = Some(unsigned as u16),
            0x28 => {
                for usage in range.take().unwrap()..=unsigned as u16 {
                    usages.push((page, usage));
                }
            }
            0xa0 => {
                if depth == 0 {
                    result.collections.push(usages[0]);
                }
                depth += 1;
            }
            0xc0 => {
                assert!(depth > 0);
                depth -= 1;
            }
            0x80 => {
                if unsigned & 1 == 0 {
                    assert_eq!(id, 1);
                    assert_eq!(usages.len(), count);
                    for (index, usage) in usages.iter().enumerate() {
                        result.fields.push(Field {
                            usage: *usage,
                            offset: result.input[id] + index * size,
                            width: size,
                            min,
                            max,
                            flags: unsigned,
                        });
                    }
                }
                result.input[id] += size * count;
            }
            0x90 => result.output[id] += size * count,
            0xb0 => result.feature[id] += size * count,
            _ => (),
        }
        if tag & 0x0c == 0 {
            usages.clear();
            range = None;
        }
        pos += 1 + n;
    }
    assert_eq!(depth, 0);
    result
}
fn value(field: &Field, report: &[u8]) -> i32 {
    let mut raw = 0u32;
    for bit in 0..field.width {
        let offset = 8 + field.offset + bit;
        raw |= u32::from((report[offset / 8] >> (offset % 8)) & 1) << bit;
    }
    if field.min < 0 {
        ((raw << (32 - field.width)) as i32) >> (32 - field.width)
    } else {
        raw as i32
    }
}
fn personas() -> [(Profile, [(u16, u16); 6]); 4] {
    [
        (
            Profile::GenericHid,
            [
                (1, 0x30),
                (1, 0x31),
                (1, 0x33),
                (1, 0x34),
                (1, 0x32),
                (1, 0x35),
            ],
        ),
        (
            Profile::Wheel,
            [
                (2, 0xc8),
                (1, 0x31),
                (1, 0x33),
                (1, 0x34),
                (2, 0xc4),
                (2, 0xc5),
            ],
        ),
        (
            Profile::FlightStick,
            [
                (1, 0x30),
                (1, 0x31),
                (1, 0x35),
                (1, 0x33),
                (2, 0xbb),
                (2, 0xc5),
            ],
        ),
        (
            Profile::ArcadeStick,
            [
                (1, 0x30),
                (1, 0x31),
                (1, 0x33),
                (1, 0x34),
                (1, 0x32),
                (1, 0x35),
            ],
        ),
    ]
}
#[test]
fn universal_descriptors_expose_exact_controls_and_authenticated_features_only() {
    for (profile, axes) in personas() {
        let layout = parse(descriptor(profile).unwrap());
        let joystick = matches!(profile, Profile::Wheel | Profile::FlightStick);
        assert_eq!(
            layout.collections,
            [(1, if joystick { 4 } else { 5 }), (0xff53, 1)]
        );
        assert_eq!(layout.fields.len(), 39);
        assert_eq!(layout.input[1], 17 * 8);
        assert_eq!(layout.input.iter().sum::<usize>(), 17 * 8);
        assert_eq!(layout.feature[0x7a], 96 * 8);
        assert_eq!(layout.feature[0x7f], 32 * 8);
        assert_eq!(layout.feature.iter().sum::<usize>(), 128 * 8);
        assert_eq!(layout.output.iter().sum::<usize>(), 0);
        for (i, f) in layout.fields[..32].iter().enumerate() {
            assert_eq!(
                (f.usage, f.width, f.offset, f.min, f.max),
                ((9, i as u16 + 1), 1, i, 0, 1)
            );
        }
        let hat = &layout.fields[32];
        assert_eq!(
            (
                hat.usage,
                hat.width,
                hat.offset,
                hat.min,
                hat.max,
                hat.flags & 0x40
            ),
            ((1, 0x39), 4, 32, 0, 7, 0x40)
        );
        for (i, f) in layout.fields[33..].iter().enumerate() {
            assert_eq!(f.usage, axes[i]);
            assert_eq!((f.width, f.offset), (16, 40 + 16 * i));
            assert_eq!(
                (f.min, f.max),
                if i < 4 { (-32768, 32767) } else { (0, 65535) }
            );
        }
    }
    assert_eq!(
        descriptor(Profile::ArcadeStick),
        descriptor(Profile::GenericHid)
    );
}
#[test]
fn descriptor_interpreted_controls_have_independent_full_range_and_release() {
    for (profile, _) in personas() {
        let layout = parse(descriptor(profile).unwrap());
        let neutral = encode(profile, &InputState::NEUTRAL).unwrap();
        for (index, field) in layout.fields.iter().enumerate() {
            assert_eq!(value(field, &neutral), if index == 32 { 8 } else { 0 });
        }
        for axis in 0..6 {
            for extreme in if axis < 4 {
                [-32768, 32767]
            } else {
                [0, 65535]
            } {
                let mut s = InputState::NEUTRAL;
                match axis {
                    0 => s.left_x = extreme as i16,
                    1 => s.left_y = extreme as i16,
                    2 => s.right_x = extreme as i16,
                    3 => s.right_y = extreme as i16,
                    4 => s.left_trigger = extreme as u16,
                    5 => s.right_trigger = extreme as u16,
                    _ => unreachable!(),
                }
                let report = encode(profile, &s).unwrap();
                for (index, field) in layout.fields[33..].iter().enumerate() {
                    assert_eq!(
                        value(field, &report),
                        if index == axis { extreme } else { 0 }
                    );
                }
            }
        }
        for button in 0..32 {
            let s = InputState {
                buttons: 1 << button,
                ..InputState::NEUTRAL
            };
            let report = encode(profile, &s).unwrap();
            for (index, field) in layout.fields[..32].iter().enumerate() {
                assert_eq!(value(field, &report), i32::from(index == button));
            }
        }
        for hat in 0..=8 {
            let s = InputState {
                hat,
                ..InputState::NEUTRAL
            };
            assert_eq!(
                value(&layout.fields[32], &encode(profile, &s).unwrap()),
                i32::from(hat)
            );
        }
    }
}
