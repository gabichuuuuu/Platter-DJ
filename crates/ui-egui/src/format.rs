//! How numbers are written in the interface. These hold digits and signs only, so they are
//! the same in every language.

/// A time as minutes, seconds and tenths: `05:00.0`, or `-01:12.4` for time that remains.
pub fn time(seconds: f64, remaining: bool) -> String {
    let tenths = if seconds.is_finite() {
        (seconds.max(0.0) * 10.0).floor() as u64
    } else {
        0
    };
    let sign = if remaining { "-" } else { "" };
    format!(
        "{sign}{:02}:{:02}.{}",
        tenths / 600,
        tenths / 10 % 60,
        tenths % 10
    )
}

/// A duration as minutes and seconds: `06:12`.
pub fn duration(seconds: f64) -> String {
    let whole = if seconds.is_finite() {
        seconds.max(0.0).round() as u64
    } else {
        0
    };
    format!("{:02}:{:02}", whole / 60, whole % 60)
}

/// A tempo in beats per minute with two decimals: `124.00`.
/// A place in a track to the millisecond: `02:48.200`.
pub fn position(seconds: f64) -> String {
    let millis = if seconds.is_finite() {
        (seconds.max(0.0) * 1_000.0).round() as u64
    } else {
        0
    };
    format!(
        "{:02}:{:02}.{:03}",
        millis / 60_000,
        millis / 1_000 % 60,
        millis % 1_000
    )
}

pub fn bpm(value: f64) -> String {
    format!("{value:.2}")
}

/// A tempo offset in per cent with its sign: `+2.5`, `-0.8`, `0.0`.
pub fn signed_percent(fraction: f64) -> String {
    let percent = fraction * 100.0;
    if percent.abs() < 0.05 {
        "0.0".to_owned()
    } else {
        format!("{percent:+.1}")
    }
}

/// A level in decibels with one decimal and its sign: `+3.0`, `-6.5`, `0.0`.
pub fn decibels(db: f64) -> String {
    if db.abs() < 0.05 {
        "0.0".to_owned()
    } else {
        format!("{db:+.1}")
    }
}

/// A sample rate in kilohertz: `44.1`, `48`.
pub fn kilohertz(hz: u32) -> String {
    if hz.is_multiple_of(1_000) {
        (hz / 1_000).to_string()
    } else {
        format!("{:.1}", f64::from(hz) / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{bpm, decibels, duration, kilohertz, signed_percent, time};

    #[test]
    fn times_are_written_with_tenths_and_never_round_up_into_the_next_second() {
        assert_eq!(time(300.0, false), "05:00.0");
        assert_eq!(time(72.49, true), "-01:12.4");
        assert_eq!(time(59.99, false), "00:59.9");
        assert_eq!(time(6_000.0, false), "100:00.0");
        assert_eq!(time(-3.0, false), "00:00.0");
        assert_eq!(time(f64::NAN, true), "-00:00.0");
        assert_eq!(duration(372.4), "06:12");
        assert_eq!(duration(f64::INFINITY), "00:00");
    }

    #[test]
    fn offsets_carry_their_sign_and_zero_has_none() {
        assert_eq!(signed_percent(-0.008), "-0.8");
        assert_eq!(signed_percent(0.025), "+2.5");
        assert_eq!(signed_percent(0.0001), "0.0");
        assert_eq!(decibels(-6.5), "-6.5");
        assert_eq!(decibels(3.0), "+3.0");
        assert_eq!(decibels(-0.01), "0.0");
        assert_eq!(bpm(124.0), "124.00");
    }

    #[test]
    fn sample_rates_drop_a_zero_decimal() {
        assert_eq!(kilohertz(44_100), "44.1");
        assert_eq!(kilohertz(48_000), "48");
        assert_eq!(kilohertz(96_000), "96");
    }
}
