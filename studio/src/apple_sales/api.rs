use super::{
    data::{AppSales, parse_report},
    period::Frequency,
};
use flate2::read::MultiGzDecoder;
use reqwest::{StatusCode, Url, blocking::Client};
use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const SALES_ENDPOINT: &str = "https://api.appstoreconnect.apple.com/v1/salesReports";
const MAX_COMPRESSED: u64 = 32 * 1024 * 1024;
const MAX_DECODED: u64 = 128 * 1024 * 1024;

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
    cancelled: &AtomicBool,
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
    if let Some(path) = cache.as_deref()
        && !cancelled.load(Ordering::Relaxed)
        && let Some(apps) = load_cached(path, refresh)
    {
        return Ok(LoadedReport {
            apps,
            cached: true,
            cache_warning: None,
        });
    }
    check_cancelled(cancelled)?;
    let http = Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| "Could not initialize the Apple connection.".to_owned())?;
    let token = crate::apple::generate_token(credentials)?;
    let content = fetch_report(
        &http,
        SALES_ENDPOINT,
        &token,
        vendor,
        frequency,
        date,
        cancelled,
    )?;
    let apps = parse_report(&content)?;
    check_cancelled(cancelled)?;
    let cache_warning = cache.as_deref().map_or_else(
        || Some("The sales report loaded, but could not be saved to the local cache.".to_owned()),
        |path| {
            crate::storage::save_credentials(path, content.as_bytes())
                .err()
                .map(|_| {
                    "The sales report loaded, but could not be saved to the local cache.".to_owned()
                })
        },
    );
    Ok(LoadedReport {
        apps,
        cached: false,
        cache_warning,
    })
}

fn fetch_report(
    http: &Client,
    endpoint: &str,
    token: &str,
    vendor: &str,
    frequency: Frequency,
    date: time::Date,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    check_cancelled(cancelled)?;
    let mut url = Url::parse(endpoint).map_err(|_| "Invalid Apple sales report URL.".to_owned())?;
    url.query_pairs_mut()
        .append_pair("filter[reportType]", "SALES")
        .append_pair("filter[reportSubType]", "SUMMARY")
        .append_pair("filter[frequency]", frequency.api_value())
        .append_pair("filter[reportDate]", &frequency.report_date(date))
        .append_pair("filter[vendorNumber]", vendor);
    let response = http
        .get(url)
        .bearer_auth(token)
        .header(
            reqwest::header::ACCEPT,
            "application/a-gzip, text/tab-separated-values",
        )
        .send()
        .map_err(|_| {
            "Could not reach App Store Connect. Check your connection and retry.".to_owned()
        })?;
    if !response.status().is_success() {
        return Err(api_error(response.status()));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_COMPRESSED + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read Apple's sales report.".to_owned())?;
    if bytes.len() as u64 > MAX_COMPRESSED {
        return Err("Apple's sales report is larger than the supported limit.".to_owned());
    }
    check_cancelled(cancelled)?;
    let decoded = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoded = Vec::new();
        MultiGzDecoder::new(Cursor::new(bytes))
            .take(MAX_DECODED + 1)
            .read_to_end(&mut decoded)
            .map_err(|_| "Apple returned a damaged compressed sales report.".to_owned())?;
        if decoded.len() as u64 > MAX_DECODED {
            return Err(
                "Apple's expanded sales report is larger than the supported limit.".to_owned(),
            );
        }
        decoded
    } else {
        bytes
    };
    String::from_utf8(decoded)
        .map_err(|_| "Apple returned a sales report that was not valid UTF-8.".to_owned())
}

fn api_error(status: StatusCode) -> String {
    match status {
        StatusCode::BAD_REQUEST => "Apple rejected the vendor number or sales report request. Check the vendor number and refresh again.",
        StatusCode::UNAUTHORIZED => "Apple rejected the API key. Check the saved .p8 key and IDs.",
        StatusCode::FORBIDDEN => "Apple denied sales report access. Check the API key's App Store Connect permissions.",
        StatusCode::NOT_FOUND => "No sales report is available for this period. Try another period, or retry if the report is still processing.",
        StatusCode::TOO_MANY_REQUESTS => "Apple is receiving too many requests. Wait a moment and refresh again.",
        StatusCode::INTERNAL_SERVER_ERROR | StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => "Apple's sales report service is temporarily unavailable. Please retry.",
        status if status.is_server_error() => "Apple's sales report service is temporarily unavailable. Please retry.",
        _ => "Apple could not load this sales report. Please retry.",
    }.to_owned()
}

fn valid_vendor(vendor: &str) -> bool {
    !vendor.is_empty() && vendor.bytes().all(|b| b.is_ascii_digit())
}
fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Apple sales report loading cancelled.".to_owned())
    } else {
        Ok(())
    }
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
fn load_cached(path: &Path, refresh: bool) -> Option<Vec<AppSales>> {
    if refresh {
        return None;
    }
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
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    fn server(status: &str, body: &[u8]) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = body.to_vec();
        let status = status.to_owned();
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                headers.push_str(&line);
            }
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
            format!("{request}{headers}")
        });
        (format!("http://{address}/v1/salesReports"), task)
    }

    #[test]
    fn sends_scoped_sales_request_and_reads_plain_tsv() {
        let body = b"SKU\tTitle\tVersion\tProduct Type Identifier\tUnits\tCountry Code\tApple Identifier\tDevice\n";
        let (endpoint, task) = server("200 OK", body);
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let date = time::Date::from_calendar_date(2025, time::Month::February, 1).unwrap();
        let content = fetch_report(
            &http,
            &endpoint,
            "safe-token",
            "12345",
            Frequency::Monthly,
            date,
            &AtomicBool::new(false),
        )
        .unwrap();
        let request = task.join().unwrap();
        let request_lower = request.to_ascii_lowercase();
        assert_eq!(content, String::from_utf8(body.to_vec()).unwrap());
        assert!(request_lower.contains("authorization: bearer safe-token"));
        assert!(request.contains("filter%5Bfrequency%5D=MONTHLY"));
        assert!(request.contains("filter%5BreportDate%5D=2025-02"));
        assert!(request.contains("filter%5BvendorNumber%5D=12345"));
    }

    #[test]
    fn maps_errors_without_returning_response_contents() {
        let (endpoint, task) = server("403 Forbidden", b"private error detail");
        let http = Client::builder().build().unwrap();
        let date = time::Date::from_calendar_date(2025, time::Month::January, 1).unwrap();
        assert!(
            fetch_report(
                &http,
                &endpoint,
                "token",
                "123",
                Frequency::Daily,
                date,
                &AtomicBool::new(false)
            )
            .unwrap_err()
            .contains("denied sales report access")
        );
        task.join().unwrap();
    }

    #[test]
    fn reads_gzip_payloads() {
        use flate2::{Compression, write::GzEncoder};
        let report = b"SKU\tTitle\tVersion\tProduct Type Identifier\tUnits\tCountry Code\tApple Identifier\tDevice\n";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(report).unwrap();
        let (endpoint, task) = server("200 OK", &encoder.finish().unwrap());
        let http = Client::builder().build().unwrap();
        let date = time::Date::from_calendar_date(2025, time::Month::January, 1).unwrap();
        assert_eq!(
            fetch_report(
                &http,
                &endpoint,
                "token",
                "123",
                Frequency::Daily,
                date,
                &AtomicBool::new(false)
            )
            .unwrap()
            .as_bytes(),
            report
        );
        task.join().unwrap();
    }

    #[test]
    fn cache_accepts_only_valid_reports_and_refresh_bypasses_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.tsv");
        let good = b"SKU\tTitle\tVersion\tProduct Type Identifier\tUnits\tCountry Code\tApple Identifier\tDevice\n";
        fs::write(&path, good).unwrap();
        assert_eq!(load_cached(&path, false), Some(Vec::new()));
        assert_eq!(load_cached(&path, true), None);
        fs::write(&path, b"not a sales report").unwrap();
        assert_eq!(load_cached(&path, false), None);
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
}
