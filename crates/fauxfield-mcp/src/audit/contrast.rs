//! WCAG 2.x contrast maths on 8-bit sRGBA colours.

fn channel(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.039_28 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Relative luminance of an opaque colour.
pub fn luminance(c: [u8; 4]) -> f64 {
    0.2126 * channel(c[0]) + 0.7152 * channel(c[1]) + 0.0722 * channel(c[2])
}

/// Composite `fg` (with its alpha) over an opaque `bg`.
pub fn over(fg: [u8; 4], bg: [u8; 4]) -> [u8; 4] {
    let a = f64::from(fg[3]) / 255.0;
    let mix = |f: u8, b: u8| (f64::from(f) * a + f64::from(b) * (1.0 - a)).round() as u8;
    [mix(fg[0], bg[0]), mix(fg[1], bg[1]), mix(fg[2], bg[2]), 255]
}

/// Contrast ratio (1.0..=21.0) of `fg` on `bg`; `fg` alpha is composited over `bg`.
pub fn ratio(fg: [u8; 4], bg: [u8; 4]) -> f64 {
    let l1 = luminance(over(fg, bg));
    let l2 = luminance(over(bg, [0, 0, 0, 255]));
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_on_white_is_21() {
        let r = ratio([0, 0, 0, 255], [255, 255, 255, 255]);
        assert!((r - 21.0).abs() < 0.01, "{r}");
    }

    #[test]
    fn same_colour_is_1() {
        assert!((ratio([9, 9, 9, 255], [9, 9, 9, 255]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn alpha_is_composited() {
        // Fully transparent foreground is the background itself.
        assert!((ratio([255, 255, 255, 0], [10, 10, 10, 255]) - 1.0).abs() < 1e-9);
    }
}
