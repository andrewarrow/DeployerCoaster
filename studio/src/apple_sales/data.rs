use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AppSales {
    pub sku: String,
    pub title: String,
    pub apple_identifier: String,
    pub parent_identifier: String,
    pub category: String,
    pub total_units: i64,
    pub proceeds: BTreeMap<String, i128>,
    pub breakdowns: Vec<Breakdown>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Breakdown {
    pub device: String,
    pub version: String,
    pub units: i64,
    pub downloads: i64,
    pub updates: i64,
    pub redownloads: i64,
    pub other: i64,
    pub countries: BTreeSet<String>,
}

#[derive(Default)]
struct Rows {
    app: Option<AppSales>,
    breakdowns: HashMap<(String, String), Breakdown>,
}

pub(super) fn parse_report(content: &str) -> Result<Vec<AppSales>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .flexible(true)
        .from_reader(content.as_bytes());
    let headers = reader
        .headers()
        .map_err(|_| "Apple returned an invalid sales report header.".to_owned())?
        .clone();
    let mut columns = HashMap::new();
    for (index, header) in headers.iter().enumerate() {
        let normalized = header.trim_start_matches('\u{feff}').trim();
        if !normalized.is_empty() && columns.insert(normalized.to_owned(), index).is_some() {
            return Err("Apple returned a sales report with duplicate columns.".to_owned());
        }
    }
    for name in [
        "SKU",
        "Title",
        "Version",
        "Product Type Identifier",
        "Units",
        "Country Code",
        "Apple Identifier",
        "Device",
    ] {
        if !columns.contains_key(name) {
            return Err("Apple returned a sales report with missing required columns.".to_owned());
        }
    }

    let mut apps: HashMap<String, Rows> = HashMap::new();
    for record in reader.records() {
        let record =
            record.map_err(|_| "Apple returned a malformed sales report row.".to_owned())?;
        if record.len() != headers.len() {
            return Err("Apple returned a sales report with an incomplete row.".to_owned());
        }
        let get = |name: &str| -> &str {
            columns
                .get(name)
                .and_then(|&i| record.get(i))
                .unwrap_or("")
                .trim()
        };
        let sku = get("SKU").to_owned();
        let title = get("Title").to_owned();
        if sku.is_empty() || title.is_empty() {
            return Err(
                "Apple returned a sales report row without an app SKU or title.".to_owned(),
            );
        }
        let version = get("Version").to_owned();
        let kind = get("Product Type Identifier");
        let units = parse_units(get("Units"))?;
        let country = get("Country Code").to_owned();
        let device = get("Device").to_owned();
        let parent_identifier = optional_column(&columns, &record, "Parent Identifier");
        let category = optional_column(&columns, &record, "Category");

        let rows = apps.entry(sku.clone()).or_default();
        if rows.app.is_none() {
            rows.app = Some(AppSales {
                sku: sku.clone(),
                title: title.clone(),
                apple_identifier: get("Apple Identifier").to_owned(),
                parent_identifier,
                category,
                total_units: 0,
                proceeds: BTreeMap::new(),
                breakdowns: Vec::new(),
            });
        }
        let app = rows.app.as_mut().expect("app inserted above");
        if app.apple_identifier != get("Apple Identifier") {
            return Err("Apple returned conflicting app details for the same SKU.".to_owned());
        }
        app.total_units = app
            .total_units
            .checked_add(units)
            .ok_or_else(|| "Apple sales totals exceed the supported range.".to_owned())?;
        let amount = optional_column_opt(&columns, &record, "Developer Proceeds")
            .or_else(|| optional_column_opt(&columns, &record, "Developer Proceeds (per unit)"));
        if let Some(amount) = amount {
            let unit_cents = parse_money_cents(&amount)?
                .ok_or_else(|| "Apple returned an invalid proceeds amount.".to_owned())?;
            let total = unit_cents
                .checked_mul(units as i128)
                .ok_or_else(|| "Apple proceeds totals exceed the supported range.".to_owned())?;
            if let Some(currency) = optional_column_opt(&columns, &record, "Currency of Proceeds") {
                add_proceeds(&mut app.proceeds, currency, total)?;
            }
        }
        let breakdown = rows
            .breakdowns
            .entry((device.clone(), version.clone()))
            .or_insert_with(|| Breakdown {
                device,
                version,
                units: 0,
                downloads: 0,
                updates: 0,
                redownloads: 0,
                other: 0,
                countries: BTreeSet::new(),
            });
        breakdown.units = breakdown
            .units
            .checked_add(units)
            .ok_or_else(|| "Apple breakdown totals exceed the supported range.".to_owned())?;
        let bucket = match kind {
            "1" | "1F" | "1T" | "1E" | "1EP" | "1EU" | "F1" | "1-B" | "F1-B" => {
                &mut breakdown.downloads
            }
            "3" | "3F" => &mut breakdown.redownloads,
            "7" | "7F" | "7T" | "F7" => &mut breakdown.updates,
            _ => &mut breakdown.other,
        };
        *bucket = bucket
            .checked_add(units)
            .ok_or_else(|| "Apple breakdown totals exceed the supported range.".to_owned())?;
        if !country.is_empty() {
            breakdown.countries.insert(country);
        }
    }
    let mut result = Vec::with_capacity(apps.len());
    for (_, mut rows) in apps {
        let mut app = rows.app.take().expect("each group has an app");
        app.breakdowns = rows.breakdowns.into_values().collect();
        app.breakdowns.sort_by(|a, b| {
            b.units
                .cmp(&a.units)
                .then_with(|| a.device.cmp(&b.device))
                .then_with(|| a.version.cmp(&b.version))
        });
        result.push(app);
    }
    result.sort_by(|a, b| {
        b.total_units
            .cmp(&a.total_units)
            .then_with(|| a.sku.cmp(&b.sku))
    });
    Ok(result)
}

fn optional_column(
    columns: &HashMap<String, usize>,
    record: &csv::StringRecord,
    name: &str,
) -> String {
    optional_column_opt(columns, record, name).unwrap_or_default()
}
fn optional_column_opt(
    columns: &HashMap<String, usize>,
    record: &csv::StringRecord,
    name: &str,
) -> Option<String> {
    columns
        .get(name)
        .and_then(|&i| record.get(i))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}
fn add_proceeds(
    proceeds: &mut BTreeMap<String, i128>,
    currency: String,
    cents: i128,
) -> Result<(), String> {
    let entry = proceeds.entry(currency).or_default();
    *entry = entry
        .checked_add(cents)
        .ok_or_else(|| "Apple proceeds totals exceed the supported range.".to_owned())?;
    Ok(())
}
fn parse_units(value: &str) -> Result<i64, String> {
    if let Ok(units) = value.parse::<i64>() {
        return Ok(units);
    }
    if let Some((whole, fraction)) = value.split_once('.')
        && fraction == "00"
        && !whole.is_empty()
        && let Ok(units) = whole.parse::<i64>()
    {
        return Ok(units);
    }
    Err("Apple returned an invalid or nonintegral unit count.".to_owned())
}
fn parse_money_cents(value: &str) -> Result<Option<i128>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let negative = value.starts_with('-');
    let unsigned = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction))
            if fraction.len() <= 2 && fraction.bytes().all(|b| b.is_ascii_digit()) =>
        {
            (whole, fraction)
        }
        Some(_) => return Err("Apple returned an invalid proceeds amount.".to_owned()),
        None => (unsigned, ""),
    };
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Apple returned an invalid proceeds amount.".to_owned());
    }
    let whole = whole
        .parse::<i128>()
        .map_err(|_| "Apple returned an invalid proceeds amount.".to_owned())?;
    let fraction = match fraction.len() {
        0 => 0,
        1 => (fraction.as_bytes()[0] - b'0') as i128 * 10,
        _ => {
            ((fraction.as_bytes()[0] - b'0') as i128) * 10 + (fraction.as_bytes()[1] - b'0') as i128
        }
    };
    let cents = whole
        .checked_mul(100)
        .and_then(|v| v.checked_add(fraction))
        .ok_or_else(|| "Apple proceeds totals exceed the supported range.".to_owned())?;
    Ok(Some(if negative { -cents } else { cents }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "SKU\tTitle\tVersion\tProduct Type Identifier\tUnits\tCountry Code\tApple Identifier\tDevice\tDeveloper Proceeds\tCurrency of Proceeds\tParent Identifier\tCategory\n";

    #[test]
    fn aggregates_correct_event_types_and_keeps_currency_totals_separate() {
        let content = format!(
            "{HEADER}z\tZulu\t1.2\t3\t-1.00\tUS\t9\tiPhone\t0.50\tUSD\tparent\tGames\na\tAlpha\t2\t1\t2\tCA\t8\tiPad\t1.20\tUSD\t\t\na\tAlpha\t2\t7\t3.00\tUS\t8\tiPad\t2.00\tCAD\t\t\na\tAlpha\t3\t99\t4\t\t8\tiPhone\t0.10\tUSD\t\t\n"
        );
        let result = parse_report(&content).unwrap();
        assert_eq!(
            result
                .iter()
                .map(|app| app.sku.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        let alpha = &result[0];
        assert_eq!(alpha.total_units, 9);
        assert_eq!(alpha.proceeds["USD"], 280);
        assert_eq!(alpha.proceeds["CAD"], 600);
        let ipad = alpha
            .breakdowns
            .iter()
            .find(|b| b.device == "iPad")
            .unwrap();
        assert_eq!(
            (ipad.downloads, ipad.updates, ipad.redownloads, ipad.other),
            (2, 3, 0, 0)
        );
        assert_eq!(
            ipad.countries,
            BTreeSet::from(["CA".to_owned(), "US".to_owned()])
        );
        assert_eq!(result[1].breakdowns[0].redownloads, -1);
    }

    #[test]
    fn supports_bom_crlf_quotes_and_header_only_reports() {
        let report = "\u{feff}SKU\tTitle\tVersion\tProduct Type Identifier\tUnits\tCountry Code\tApple Identifier\tDevice\r\n1\t\"A\tname\"\t1\t1F\t1.00\tUS\t2\tiPhone\r\n";
        let parsed = parse_report(report).unwrap();
        assert_eq!(parsed[0].title, "A\tname");
        assert!(parse_report("SKU\tTitle\tVersion\tProduct Type Identifier\tUnits\tCountry Code\tApple Identifier\tDevice\n").unwrap().is_empty());
    }

    #[test]
    fn rejects_malformed_units_and_money() {
        let mut report = format!("{HEADER}1\tApp\t1\t1\t1.5\tUS\t2\tiPhone\t1.00\tUSD\t\t\n");
        assert!(parse_report(&report).is_err());
        report = format!("{HEADER}1\tApp\t1\t1\t1\tUS\t2\tiPhone\tNaN\tUSD\t\t\n");
        assert!(parse_report(&report).is_err());
    }

    #[test]
    fn rejects_incomplete_rows_duplicate_columns_and_conflicting_sku_identity() {
        let truncated = format!("{HEADER}1\tApp\t1\t1\t1\tUS\t2\tiPhone\n");
        assert!(parse_report(&truncated).is_err());
        let duplicate = HEADER.replacen("\tTitle\t", "\tSKU\tTitle\t", 1);
        assert!(parse_report(&duplicate).is_err());
        let localized = format!(
            "{HEADER}1\tApp\t1\t1\t1\tUS\t2\tiPhone\t\t\t\t\n1\tLocalized title\t1\t1\t1\tUS\t2\tiPhone\t\t\t\t\n"
        );
        let app = parse_report(&localized).unwrap().remove(0);
        assert_eq!(app.title, "App");
        assert_eq!(app.total_units, 2);
        let conflicting = format!(
            "{HEADER}1\tApp\t1\t1\t1\tUS\t2\tiPhone\t\t\t\t\n1\tApp\t1\t1\t1\tUS\t3\tiPhone\t\t\t\t\n"
        );
        assert!(parse_report(&conflicting).is_err());
    }
}
