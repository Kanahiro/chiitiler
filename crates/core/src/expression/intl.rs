//! Minimal `Intl.Collator` / `Intl.NumberFormat` stand-ins. Only English (root
//! collation, en-US number formatting) is supported; other locales resolve to `en-US`.

use std::cmp::Ordering;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

const DEFAULT_LOCALE: &str = "en-US";

/// Resolve a requested BCP 47 tag against the supported set (English only).
fn resolve_locale(requested: Option<&str>) -> String {
    match requested {
        Some(l) if l.split(['-', '_']).next().is_some_and(|lang| lang.eq_ignore_ascii_case("en")) => {
            let mut parts = l.split(['-', '_']);
            let lang = parts.next().unwrap().to_ascii_lowercase();
            match parts.next() {
                Some(region) if region.len() == 2 => format!("{lang}-{}", region.to_ascii_uppercase()),
                _ => lang,
            }
        }
        _ => DEFAULT_LOCALE.into(),
    }
}

#[derive(Debug, Clone)]
pub struct Collator {
    pub case_sensitive: bool,
    pub diacritic_sensitive: bool,
    pub locale: Option<String>,
}

impl Collator {
    pub fn sensitivity(&self) -> &'static str {
        match (self.case_sensitive, self.diacritic_sensitive) {
            (true, true) => "variant",
            (true, false) => "case",
            (false, true) => "accent",
            (false, false) => "base",
        }
    }

    pub fn resolved_locale(&self) -> String {
        resolve_locale(self.locale.as_deref())
    }

    /// Multi-level comparison: base letters, then accents, then case (lowercase first).
    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        let base = |s: &str| -> String { s.nfd().filter(|c| !is_combining_mark(*c)).collect::<String>().to_lowercase() };
        let ord = base(a).cmp(&base(b));
        if ord != Ordering::Equal {
            return ord;
        }
        if self.diacritic_sensitive {
            let accents = |s: &str| -> String { s.nfd().collect::<String>().to_lowercase() };
            let ord = accents(a).cmp(&accents(b));
            if ord != Ordering::Equal {
                return ord;
            }
        }
        if self.case_sensitive {
            let strip = |s: &str| -> Vec<char> { s.nfd().filter(|c| !is_combining_mark(*c)).collect() };
            for (x, y) in strip(a).into_iter().zip(strip(b)) {
                if x != y {
                    // Same base letter here, so they differ only by case.
                    return if x.is_lowercase() { Ordering::Less } else { Ordering::Greater };
                }
            }
        }
        Ordering::Equal
    }
}

pub struct NumberFormatOptions<'a> {
    pub locale: Option<&'a str>,
    pub currency: Option<&'a str>,
    pub unit: Option<&'a str>,
    pub min_fraction_digits: Option<f64>,
    pub max_fraction_digits: Option<f64>,
}

pub fn format_number(n: f64, o: &NumberFormatOptions) -> String {
    let _ = resolve_locale(o.locale);
    if n.is_nan() {
        return "NaN".into();
    }
    let currency_digits = o.currency.map(|c| if matches!(c.to_ascii_uppercase().as_str(), "JPY" | "KRW") { 0 } else { 2 });
    let default_min = currency_digits.unwrap_or(0);
    let default_max = currency_digits.unwrap_or(3);
    let min = o.min_fraction_digits.map_or(default_min, |v| v as usize);
    let max = o.max_fraction_digits.map_or(default_max.max(min), |v| v as usize).max(min);

    let body = if n.is_infinite() { "∞".to_string() } else { group(&round_half_expand(n.abs(), max), min) };
    let sign = if n < 0.0 { "-" } else { "" };
    if let Some(code) = o.currency {
        let code = code.to_ascii_uppercase();
        return match code.as_str() {
            "USD" => format!("{sign}${body}"),
            "EUR" => format!("{sign}€{body}"),
            "JPY" => format!("{sign}¥{body}"),
            "GBP" => format!("{sign}£{body}"),
            _ => format!("{sign}{code}\u{a0}{body}"),
        };
    }
    if let Some(unit) = o.unit {
        let (sym, space) = match unit {
            "meter" => ("m", true),
            "kilometer" => ("km", true),
            "centimeter" => ("cm", true),
            "millimeter" => ("mm", true),
            "mile" => ("mi", true),
            "foot" => ("ft", true),
            "inch" => ("in", true),
            "yard" => ("yd", true),
            "kilogram" => ("kg", true),
            "gram" => ("g", true),
            "second" => ("sec", true),
            "minute" => ("min", true),
            "hour" => ("hr", true),
            "byte" => ("byte", true),
            "kilobyte" => ("kB", true),
            "megabyte" => ("MB", true),
            "gigabyte" => ("GB", true),
            "celsius" => ("°C", false),
            "fahrenheit" => ("°F", false),
            "degree" => ("deg", true),
            "percent" => ("%", false),
            other => (other, true),
        };
        return format!("{sign}{body}{}{sym}", if space { " " } else { "" });
    }
    format!("{sign}{body}")
}

/// Fixed-point string of `x` (>= 0) with `digits` fraction digits. Like ICU, rounds the
/// shortest round-trip decimal representation, ties away from zero.
fn round_half_expand(x: f64, digits: usize) -> String {
    let e = format!("{x:e}");
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i64 = exp.parse().unwrap();
    let sig: Vec<u8> = mantissa.bytes().filter(|c| *c != b'.').map(|c| c - b'0').collect();
    // Digits of x as integer part and fraction part, padded.
    let point = exp + 1; // number of integer digits (may be <= 0)
    let mut int: Vec<u8> = Vec::new();
    let mut frac: Vec<u8> = Vec::new();
    for (i, &d) in sig.iter().enumerate() {
        if (i as i64) < point { int.push(d) } else { frac.push(d) }
    }
    while (int.len() as i64) < point {
        int.push(0);
    }
    if point < 0 {
        frac.splice(0..0, std::iter::repeat_n(0, (-point) as usize));
    }
    if int.is_empty() {
        int.push(0);
    }
    let round_up = frac.get(digits).is_some_and(|&d| d >= 5);
    frac.resize(digits, 0);
    let mut d: Vec<u8> = int.iter().chain(frac.iter()).copied().collect();
    if round_up {
        let mut i = d.len();
        loop {
            if i == 0 {
                d.insert(0, 1);
                break;
            }
            i -= 1;
            if d[i] == 9 {
                d[i] = 0;
            } else {
                d[i] += 1;
                break;
            }
        }
    }
    let s: String = d.iter().map(|&c| (c + b'0') as char).collect();
    let split = s.len() - digits;
    if digits == 0 { s } else { format!("{}.{}", &s[..split], &s[split..]) }
}

/// Group thousands with "," and trim trailing fraction zeros down to `min` digits.
fn group(fixed: &str, min: usize) -> String {
    let (int, frac) = fixed.split_once('.').unwrap_or((fixed, ""));
    let mut frac = frac.to_string();
    while frac.len() > min && frac.ends_with('0') {
        frac.pop();
    }
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if !frac.is_empty() {
        out.push('.');
        out.push_str(&frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_format() {
        let o = |min, max| NumberFormatOptions { locale: None, currency: None, unit: None, min_fraction_digits: min, max_fraction_digits: max };
        assert_eq!(format_number(123456.789, &o(None, None)), "123,456.789");
        assert_eq!(format_number(987654321.234567, &o(Some(2.0), Some(4.0))), "987,654,321.2346");
        assert_eq!(format_number(0.5, &o(None, Some(0.0))), "1");
        assert_eq!(format_number(-1234.0, &o(None, None)), "-1,234");
        assert_eq!(format_number(987654321.234567, &o(Some(15.0), Some(20.0))), "987,654,321.234567000000000");
        assert_eq!(format_number(1.005, &o(None, Some(2.0))), "1.01");
        assert_eq!(format_number(0.0001, &o(None, None)), "0");
    }

    #[test]
    fn collator() {
        let c = |cs, ds| Collator { case_sensitive: cs, diacritic_sensitive: ds, locale: None };
        assert_eq!(c(false, false).compare("ä", "A"), Ordering::Equal);
        assert_eq!(c(false, true).compare("ä", "a"), Ordering::Greater);
        assert_eq!(c(true, false).compare("a", "A"), Ordering::Less);
        assert_eq!(c(false, false).compare("a", "B"), Ordering::Less);
    }
}
