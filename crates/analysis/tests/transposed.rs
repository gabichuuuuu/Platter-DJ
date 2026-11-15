//! A key moved by semitones: `docs/behavior/keylock.md`, The resulting key.

use platter_analysis::{Key, Mode, Transposed};

const A_MINOR: Key = Key {
    tonic: 9,
    mode: Mode::Minor,
};

fn minor(tonic: u8) -> Key {
    Key {
        tonic,
        mode: Mode::Minor,
    }
}

#[test]
fn whole_semitones_move_the_tonic_around_the_octave_and_keep_the_mode() {
    let cases = [
        (0.0, 9),
        (3.0, 0),
        (-9.0, 0),
        (12.0, 9),
        (-12.0, 9),
        (-13.0, 8),
    ];
    for (semitones, tonic) in cases {
        assert_eq!(
            A_MINOR.transposed(semitones),
            Some(Transposed {
                key: minor(tonic),
                cents: 0.0
            }),
            "{semitones}"
        );
    }
}

#[test]
fn a_fraction_gives_the_nearest_key_and_the_cents_it_is_off() {
    let found = A_MINOR.transposed(1.3).unwrap();
    assert_eq!(found.key, minor(10));
    assert!((found.cents - 30.0).abs() < 1e-9, "{found:?}");
    let found = A_MINOR.transposed(-0.7).unwrap();
    assert_eq!(found.key, minor(8));
    assert!((found.cents - 30.0).abs() < 1e-9, "{found:?}");
    let found = A_MINOR.transposed(0.4).unwrap();
    assert_eq!(found.key, A_MINOR);
    assert!((found.cents - 40.0).abs() < 1e-9, "{found:?}");
}

#[test]
fn a_shift_that_is_not_a_number_has_no_key() {
    for semitones in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(A_MINOR.transposed(semitones), None);
    }
}
