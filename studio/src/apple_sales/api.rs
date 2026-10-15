use super::{
    data::{AppSales, parse_report},
    period::Frequency,
};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_DECODED: u64 = 128 * 1024 * 1024;
const LIVE_UNAVAILABLE: &str =
    "Live sales reports are unavailable because Parsequerade does not support sales yet.";

pub(super) struct LoadedReport {
    pub apps: Vec<AppSales>,
    pub cached: bool,
    pub cache_warning: Option<String>,
}

pub(super) fn load_report(
    credentials: &crate::apple::AppleCredentials,
    vendor: &str,
    frequency: Frequency,
    date: time::Date,
    refresh: bool,
) -> Result<LoadedReport, String> {
    if !valid_vendor(vendor) {
        return Err("Apple vendor number is invalid.".to_owned());
    }
    let cache = dirs::cache_dir().and_then(|root| {
        cache_path(
            &root,
            &credentials.sales_cache_account(),
            vendor,
            frequency,
            date,
        )
        .ok()
    });
    if !refresh
        && let Some(path) = cache.as_deref()
        && let Some(apps) = load_cached(path)
    {
        return Ok(LoadedReport {
            apps,
            cached: true,
            cache_warning: None,
        });
    }
    Err(LIVE_UNAVAILABLE.to_owned())
}

fn valid_vendor(vendor: &str) -> bool {
    !vendor.is_empty() && vendor.bytes().all(|b| b.is_ascii_digit())
}

fn cache_path(
    root: &Path,
    account: &str,
    vendor: &str,
    frequency: Frequency,
    date: time::Date,
) -> Result<PathBuf, String> {
    if !valid_vendor(vendor) {
        return Err("Apple vendor number is invalid.".to_owned());
    }
    let period = match frequency {
        Frequency::Daily => format!("day_{}.tsv", frequency.report_date(date)),
        Frequency::Monthly => format!("month_{}.tsv", frequency.report_date(date)),
    };
    Ok(root
        .join("DeployerCoaster")
        .join("sales")
        .join(account)
        .join(vendor)
        .join(period))
}

fn load_cached(path: &Path) -> Option<Vec<AppSales>> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_DECODED + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_DECODED {
        return None;
    }
    let content = String::from_utf8(bytes).ok()?;
    parse_report(&content).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_accepts_only_valid_reports() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.tsv");
        let good = b"SKU\tTitle\tVersion\tProduct Type Identifier\tUnits\tCountry Code\tApple Identifier\tDevice\n";
        fs::write(&path, good).unwrap();
        assert_eq!(load_cached(&path), Some(Vec::new()));
        fs::write(&path, b"not a sales report").unwrap();
        assert_eq!(load_cached(&path), None);
    }

    #[test]
    fn cache_paths_are_vendor_and_period_scoped() {
        let root = Path::new("/tmp/cache");
        let date = time::Date::from_calendar_date(2024, time::Month::February, 29).unwrap();
        assert!(
            cache_path(root, "account-a", "123", Frequency::Daily, date)
                .unwrap()
                .ends_with("account-a/123/day_2024-02-29.tsv")
        );
        assert_ne!(
            cache_path(root, "account-a", "123", Frequency::Monthly, date),
            cache_path(root, "account-b", "123", Frequency::Monthly, date)
        );
        assert!(cache_path(root, "account-a", "../123", Frequency::Monthly, date).is_err());
    }

    #[test]
    fn unavailable_live_reads_have_an_explicit_parsequerade_reason() {
        assert!(LIVE_UNAVAILABLE.contains("Parsequerade"));
        assert!(LIVE_UNAVAILABLE.contains("sales"));
    }
}
