//! WCAG 2.x contrast ratio.

use inert_proto::Rgb;

pub const MIN_TEXT_CONTRAST: f64 = 4.5;

fn channel(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn luminance(c: Rgb) -> f64 {
    0.2126 * channel(c.0) + 0.7152 * channel(c.1) + 0.0722 * channel(c.2)
}

pub fn ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ratios() {
        assert!((ratio(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
        assert!((ratio(Rgb(1, 1, 1), Rgb(1, 1, 1)) - 1.0).abs() < 1e-9);
        // #767676 on white is the classic 4.54:1.
        assert!(ratio(Rgb(0x76, 0x76, 0x76), Rgb(255, 255, 255)) > 4.5);
        assert!(ratio(Rgb(0x77, 0x77, 0x77), Rgb(255, 255, 255)) < 4.5);
    }
}
