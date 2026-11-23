//! Currencies and number entry.

pub struct Currency {
    pub code: &'static str,
    pub symbol: &'static str,
    pub decimals: u32,
}

pub const CURRENCIES: &[Currency] = &[
    Currency { code: "USD", symbol: "$", decimals: 2 },
    Currency { code: "CAD", symbol: "$", decimals: 2 },
    Currency { code: "MXN", symbol: "$", decimals: 2 },
    Currency { code: "EUR", symbol: "€", decimals: 2 },
    Currency { code: "GBP", symbol: "£", decimals: 2 },
    Currency { code: "AUD", symbol: "$", decimals: 2 },
    Currency { code: "NZD", symbol: "$", decimals: 2 },
    Currency { code: "CHF", symbol: "CHF ", decimals: 2 },
    Currency { code: "SEK", symbol: "kr ", decimals: 2 },
    Currency { code: "NOK", symbol: "kr ", decimals: 2 },
    Currency { code: "DKK", symbol: "kr ", decimals: 2 },
    Currency { code: "PLN", symbol: "zł ", decimals: 2 },
    Currency { code: "INR", symbol: "₹", decimals: 2 },
    Currency { code: "CNY", symbol: "¥", decimals: 2 },
    Currency { code: "JPY", symbol: "¥", decimals: 0 },
    Currency { code: "KRW", symbol: "₩", decimals: 0 },
    Currency { code: "BRL", symbol: "R$", decimals: 2 },
    Currency { code: "ZAR", symbol: "R ", decimals: 2 },
];

pub const DEFAULT_CURRENCY: &str = "USD";

pub fn currency(code: &str) -> &'static Currency {
    CURRENCIES
        .iter()
        .find(|currency| currency.code == code)
        .unwrap_or(&CURRENCIES[0])
}

impl Currency {
    pub fn format(&self, amount: f64) -> String {
        let digits = format!("{:.*}", self.decimals as usize, amount.abs());
        let sign = if amount < 0.0 && digits.chars().any(|c| c.is_ascii_digit() && c != '0') { "-" } else { "" };
        format!("{sign}{}{}", self.symbol, group_thousands(&digits))
    }
}

fn group_thousands(digits: &str) -> String {
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (digits, None),
    };
    let mut grouped = String::new();
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    match fraction {
        Some(fraction) => format!("{grouped}.{fraction}"),
        None => grouped,
    }
}

/// Reads a number typed by the user. Currency symbols, spaces and thousands separators are ignored.
pub fn parse_number(text: &str) -> Option<f64> {
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<f64>().ok().filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_dollars() {
        assert_eq!(currency("USD").format(3.5), "$3.50");
        assert_eq!(currency("USD").format(1234.5), "$1,234.50");
        assert_eq!(currency("JPY").format(398.0), "¥398");
    }

    #[test]
    fn parses_typed_prices() {
        assert_eq!(parse_number("$4.49"), Some(4.49));
        assert_eq!(parse_number(" 1,299.00 "), Some(1299.0));
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("1.2.3"), None);
    }
}
