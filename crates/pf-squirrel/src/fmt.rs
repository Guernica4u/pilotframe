//! Number printing as the game does it: the C library `printf` of the
//! Microsoft compiler the game was built with (oracle F001–F031):
//! - `%g` with 6 significant digits by default;
//! - exponents with at least three digits (`1e+010`);
//! - infinity as `1.#INF`, not-a-number as `-1.#IND` / `1.#QNAN`;
//! - ties rounded away from zero (that compiler's behaviour before 2015;
//!   still to be checked with an oracle case).
//!
//! Floats are 32-bit in scripts and widened to 64-bit for printing, as C
//! does when passing a `float` to `printf`.

/// A float as `print` and string concatenation show it (`%g`).
pub fn float_to_string(x: f32) -> String {
    format_float(x as f64, b'g', None, &Flags::default())
}

#[derive(Default, Clone, Copy)]
pub struct Flags {
    pub left: bool,
    pub plus: bool,
    pub space: bool,
    pub alt: bool,
    pub zero: bool,
}

/// `%f`, `%e`, `%E`, `%g` or `%G` with an optional precision (no width).
pub fn format_float(x: f64, conv: u8, precision: Option<usize>, flags: &Flags) -> String {
    let sign = if x.is_sign_negative() {
        "-"
    } else if flags.plus {
        "+"
    } else if flags.space {
        " "
    } else {
        ""
    };
    if !x.is_finite() {
        return format!("{sign}{}", special(x, conv, precision));
    }
    let x = x.abs();
    let upper = conv.is_ascii_uppercase();
    let body = match conv.to_ascii_lowercase() {
        b'f' => fixed(x, precision.unwrap_or(6), flags.alt),
        b'e' => scientific(x, precision.unwrap_or(6), upper, flags.alt),
        _ => general(x, precision.unwrap_or(6), upper, flags.alt),
    };
    format!("{sign}{body}")
}

/// The old Microsoft forms for infinity and not-a-number. `%f` pads them
/// to the precision like digits (`1.#INF00`).
fn special(x: f64, conv: u8, precision: Option<usize>) -> String {
    let word = if x.is_infinite() {
        "1.#INF"
    } else if x.is_sign_negative() {
        "1.#IND"
    } else {
        "1.#QNAN"
    };
    match conv.to_ascii_lowercase() {
        b'g' => word.to_string(),
        _ => {
            let p = precision.unwrap_or(6);
            let digits = word.len() - 2;
            let mut s = word.to_string();
            while s.len() - 2 < p.max(digits) {
                s.push('0');
            }
            if conv.eq_ignore_ascii_case(&b'e') {
                s.push_str("e+000");
            }
            s
        }
    }
}

/// Exact decimal digits of `x` (positive, finite) rounded to `frac` places,
/// ties away from zero. Returns (integer digits, fraction digits).
fn round_fixed(x: f64, frac: usize) -> (String, String) {
    // Rust prints the exact value to any precision; 25 extra places show
    // which way to round (and whether it's a tie).
    let s = format!("{:.*}", frac + 25, x);
    let (int, rest) = s.split_once('.').unwrap_or((&s, ""));
    let mut digits: Vec<u8> = int.bytes().chain(rest.bytes().take(frac)).collect();
    if rest.as_bytes().get(frac).is_some_and(|&d| d >= b'5') {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, b'1');
                break;
            }
            i -= 1;
            if digits[i] == b'9' {
                digits[i] = b'0';
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let split = digits.len() - frac;
    let int = String::from_utf8(digits[..split].to_vec()).unwrap_or_default();
    let fr = String::from_utf8(digits[split..].to_vec()).unwrap_or_default();
    (int, fr)
}

fn fixed(x: f64, prec: usize, alt: bool) -> String {
    let (int, frac) = round_fixed(x, prec);
    if prec == 0 && !alt {
        int
    } else {
        format!("{int}.{frac}")
    }
}

/// `x` (positive, finite) as one digit, `prec` more, and a power of ten,
/// rounded ties away from zero.
fn round_scientific(x: f64, prec: usize) -> (String, i32) {
    if x == 0.0 {
        return ("0".repeat(prec + 1), 0);
    }
    let s = format!("{:.*e}", prec + 25, x);
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let mut exp: i32 = exp.parse().unwrap_or(0);
    let all: Vec<u8> = mant.bytes().filter(|b| b.is_ascii_digit()).collect();
    let mut digits = all[..=prec].to_vec();
    if all.get(prec + 1).is_some_and(|&d| d >= b'5') {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, b'1');
                digits.pop();
                exp += 1;
                break;
            }
            i -= 1;
            if digits[i] == b'9' {
                digits[i] = b'0';
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    (String::from_utf8(digits).unwrap_or_default(), exp)
}

fn exponent(e: i32, upper: bool) -> String {
    let sign = if e < 0 { '-' } else { '+' };
    format!("{}{sign}{:03}", if upper { 'E' } else { 'e' }, e.abs())
}

fn scientific(x: f64, prec: usize, upper: bool, alt: bool) -> String {
    let (digits, e) = round_scientific(x, prec);
    let mut s = digits[..1].to_string();
    if prec > 0 || alt {
        s.push('.');
        s.push_str(&digits[1..]);
    }
    s + &exponent(e, upper)
}

fn general(x: f64, prec: usize, upper: bool, alt: bool) -> String {
    let p = prec.max(1);
    let (_, e) = round_scientific(x, p - 1);
    let mut s = if e < -4 || e >= p as i32 {
        let (digits, e) = round_scientific(x, p - 1);
        let mut m = digits[..1].to_string();
        if p > 1 || alt {
            m.push('.');
            m.push_str(&digits[1..]);
        }
        if !alt {
            m = strip_zeros(m);
        }
        return m + &exponent(e, upper);
    } else {
        fixed(x, (p as i32 - 1 - e) as usize, alt)
    };
    if !alt {
        s = strip_zeros(s);
    }
    s
}

fn strip_zeros(s: String) -> String {
    if !s.contains('.') {
        return s;
    }
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // What the game printed (oracle run 1).
    #[test]
    fn matches_the_game() {
        let g = |x: f32| float_to_string(x);
        assert_eq!(g(1.0 / 3.0), "0.333333");
        assert_eq!(g(2.0 / 3.0), "0.666667");
        assert_eq!(g(0.1 + 0.2), "0.3");
        assert_eq!(g(1.0), "1");
        assert_eq!(g(100.0), "100");
        assert_eq!(g(1.0e10), "1e+010");
        assert_eq!(g(1.0e-5), "1e-005");
        assert_eq!(g(123456789.0), "1.23457e+008");
        assert_eq!(g(-0.0), "-0");
        assert_eq!(g(16777217.0), "1.67772e+007");
        assert_eq!(g(f32::INFINITY), "1.#INF");
        let f = Flags::default();
        assert_eq!(format_float(1.5, b'f', None, &f), "1.500000");
        assert_eq!(format_float(2.345f32 as f64, b'f', Some(2), &f), "2.35");
        assert_eq!(format_float(0.0001f32 as f64, b'g', None, &f), "0.0001");
        assert_eq!(
            format_float(12345.678f32 as f64, b'e', None, &f),
            "1.234568e+004"
        );
    }
}
