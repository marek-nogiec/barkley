//! Barkley plugin: unit conversion. `12 km to mi`, `100 f in c`, `2 gb to mb`.
//! Sees every keyword-less query, so it stays tiny and returns nothing fast.

use barkley_plugin::Item;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Length,
    Mass,
    Volume,
    Temperature,
    Data,
}

/// `base = value * factor + offset`. Offset is only non-zero for temperatures.
struct Unit {
    symbol: &'static str,
    aliases: &'static [&'static str],
    family: Family,
    factor: f64,
    offset: f64,
}

const fn u(symbol: &'static str, aliases: &'static [&'static str], family: Family, factor: f64) -> Unit {
    Unit { symbol, aliases, family, factor, offset: 0.0 }
}

use Family::*;

// Bases: metre, gram, litre, kelvin, byte.
static UNITS: &[Unit] = &[
    u("mm", &["millimeter", "millimeters", "millimetre", "millimetres"], Length, 0.001),
    u("cm", &["centimeter", "centimeters", "centimetre", "centimetres"], Length, 0.01),
    u("m", &["meter", "meters", "metre", "metres"], Length, 1.0),
    u("km", &["kilometer", "kilometers", "kilometre", "kilometres", "kms"], Length, 1000.0),
    u("in", &["inch", "inches", "\""], Length, 0.0254),
    u("ft", &["foot", "feet", "'"], Length, 0.3048),
    u("yd", &["yard", "yards", "yds"], Length, 0.9144),
    u("mi", &["mile", "miles"], Length, 1609.344),
    u("nmi", &["nautical mile", "nautical miles"], Length, 1852.0),
    u("mg", &["milligram", "milligrams"], Mass, 0.001),
    u("g", &["gram", "grams"], Mass, 1.0),
    u("kg", &["kilogram", "kilograms", "kilo", "kilos", "kgs"], Mass, 1000.0),
    u("t", &["tonne", "tonnes", "ton", "tons"], Mass, 1_000_000.0),
    u("oz", &["ounce", "ounces"], Mass, 28.349_523_125),
    u("lb", &["lbs", "pound", "pounds"], Mass, 453.592_37),
    u("st", &["stone", "stones"], Mass, 6_350.293_18),
    u("ml", &["milliliter", "milliliters", "millilitre", "millilitres"], Volume, 0.001),
    u("cl", &["centiliter", "centiliters", "centilitre", "centilitres"], Volume, 0.01),
    u("dl", &["deciliter", "deciliters", "decilitre", "decilitres"], Volume, 0.1),
    u("l", &["liter", "liters", "litre", "litres", "ltr"], Volume, 1.0),
    u("tsp", &["teaspoon", "teaspoons"], Volume, 0.004_928_921_593_75),
    u("tbsp", &["tablespoon", "tablespoons"], Volume, 0.014_786_764_781_25),
    u("floz", &["fl oz", "fluid ounce", "fluid ounces"], Volume, 0.029_573_529_562_5),
    u("cup", &["cups"], Volume, 0.236_588_236_5),
    u("pt", &["pint", "pints"], Volume, 0.473_176_473),
    u("qt", &["quart", "quarts"], Volume, 0.946_352_946),
    u("gal", &["gallon", "gallons"], Volume, 3.785_411_784),
    Unit { symbol: "°C", aliases: &["c", "celsius", "°c", "degc"], family: Temperature, factor: 1.0, offset: 273.15 },
    Unit {
        symbol: "°F",
        aliases: &["f", "fahrenheit", "°f", "degf"],
        family: Temperature,
        factor: 5.0 / 9.0,
        offset: 459.67 * 5.0 / 9.0,
    },
    u("K", &["k", "kelvin", "kelvins"], Temperature, 1.0),
    u("B", &["b", "byte", "bytes"], Data, 1.0),
    u("KB", &["kb", "kilobyte", "kilobytes"], Data, 1e3),
    u("MB", &["mb", "megabyte", "megabytes"], Data, 1e6),
    u("GB", &["gb", "gigabyte", "gigabytes"], Data, 1e9),
    u("TB", &["tb", "terabyte", "terabytes"], Data, 1e12),
    u("PB", &["pb", "petabyte", "petabytes"], Data, 1e15),
    u("KiB", &["kib", "kibibyte", "kibibytes"], Data, 1024.0),
    u("MiB", &["mib", "mebibyte", "mebibytes"], Data, 1024.0 * 1024.0),
    u("GiB", &["gib", "gibibyte", "gibibytes"], Data, 1024.0 * 1024.0 * 1024.0),
    u("TiB", &["tib", "tebibyte", "tebibytes"], Data, 1024.0 * 1024.0 * 1024.0 * 1024.0),
];

const SEPARATORS: &[&str] = &["to", "in", "as", "=", "->"];

fn find_unit(name: &str) -> Option<&'static Unit> {
    let name = name.trim();
    let lower = name.to_lowercase();
    UNITS.iter().find(|u| u.symbol == name || u.symbol.to_lowercase() == lower || u.aliases.contains(&lower.as_str()))
}

/// "10km", "10 km", "-40 °f", "1,000 ft" → (value, unit)
fn parse_quantity(s: &str) -> Option<(f64, &'static Unit)> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+'))).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let value: f64 = num.replace(',', "").parse().ok()?;
    Some((value, find_unit(unit)?))
}

/// Parse "<qty> <sep> <unit>", trying every separator position so that
/// "10 in in cm" resolves to inches → centimetres.
fn parse(text: &str) -> Option<(f64, &'static Unit, &'static Unit)> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    (1..tokens.len().saturating_sub(1)).find_map(|i| {
        if !SEPARATORS.contains(&tokens[i].to_lowercase().as_str()) {
            return None;
        }
        let (value, from) = parse_quantity(&tokens[..i].join(" "))?;
        let to = find_unit(&tokens[i + 1..].join(" "))?;
        (from.family == to.family).then_some((value, from, to))
    })
}

fn convert(value: f64, from: &Unit, to: &Unit) -> f64 {
    let base = value * from.factor + from.offset;
    (base - to.offset) / to.factor
}

/// Two decimals for ordinary magnitudes, four significant digits for tiny ones,
/// trailing zeros trimmed.
pub fn format_number(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let decimals = if v.abs() >= 1.0 { 2 } else { (3 - v.abs().log10().floor() as i32).clamp(2, 12) as usize };
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".into() } else { s }
}

fn items(query: &str) -> Vec<Item> {
    let Some((value, from, to)) = parse(query.trim()) else {
        return Vec::new();
    };
    let result = format_number(convert(value, from, to));
    vec![
        Item::new(format!("{result} {}", to.symbol))
            .subtitle(format!("{} {} = {result} {}", format_number(value), from.symbol, to.symbol))
            .copy(result)
            .score(1000)
            .symbol(match to.family {
                Length => "ruler",
                Mass => "scalemass",
                Volume => "drop",
                Temperature => "thermometer.medium",
                Data => "internaldrive",
            }),
    ]
}

fn main() {
    barkley_plugin::run(items);
}

#[cfg(test)]
mod tests {
    use super::*;
    use barkley_plugin::Action;

    fn conv(q: &str) -> Option<String> {
        items(q).into_iter().next().map(|i| i.title)
    }

    #[test]
    fn length() {
        assert_eq!(conv("10 km to mi").as_deref(), Some("6.21 mi"));
        assert_eq!(conv("12km to mi").as_deref(), Some("7.46 mi"));
        assert_eq!(conv("1 mi in m").as_deref(), Some("1609.34 m"));
        assert_eq!(conv("10 in in cm").as_deref(), Some("25.4 cm"));
        assert_eq!(conv("6 feet to meters").as_deref(), Some("1.83 m"));
    }

    #[test]
    fn mass() {
        assert_eq!(conv("1 kg to lb").as_deref(), Some("2.2 lb"));
        assert_eq!(conv("16 oz to g").as_deref(), Some("453.59 g"));
        assert_eq!(conv("1 st in kg").as_deref(), Some("6.35 kg"));
    }

    #[test]
    fn temperature() {
        assert_eq!(conv("100 f in c").as_deref(), Some("37.78 °C"));
        assert_eq!(conv("-40 c to f").as_deref(), Some("-40 °F"));
        assert_eq!(conv("0 k to c").as_deref(), Some("-273.15 °C"));
        assert_eq!(conv("212 °F to celsius").as_deref(), Some("100 °C"));
    }

    #[test]
    fn volume() {
        assert_eq!(conv("1 gal to l").as_deref(), Some("3.79 l"));
        assert_eq!(conv("250 ml to cups").as_deref(), Some("1.06 cup"));
        assert_eq!(conv("1 tbsp to tsp").as_deref(), Some("3 tsp"));
    }

    #[test]
    fn data() {
        assert_eq!(conv("2 gb to mb").as_deref(), Some("2000 MB"));
        assert_eq!(conv("1 GiB to MB").as_deref(), Some("1073.74 MB"));
        assert_eq!(conv("1,024 KiB as MiB").as_deref(), Some("1 MiB"));
    }

    #[test]
    fn small_values_keep_significant_digits() {
        assert_eq!(conv("1 mm to mi").as_deref(), Some("0.0000006214 mi"));
        assert_eq!(format_number(0.5), "0.5");
        assert_eq!(format_number(0.012345), "0.01235");
    }

    #[test]
    fn copies_bare_number() {
        let item = items("10 km to mi").remove(0);
        assert_eq!(item.action, Some(Action::Copy { text: "6.21".into() }));
    }

    #[test]
    fn rejects_non_conversions() {
        assert_eq!(conv("rust lang"), None);
        assert_eq!(conv("10 km to kg"), None);
        assert_eq!(conv("km to mi"), None);
        assert_eq!(conv("10 km to"), None);
        assert_eq!(conv("10 parsecs to km"), None);
    }
}
