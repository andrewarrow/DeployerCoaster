use super::*;

pub(super) fn list_apps(
    http: &Client,
    session: &mut Session,
    cancelled: &AtomicBool,
    endpoint: &str,
) -> Result<Vec<PlayApp>, String> {
    if session.expires_at <= Instant::now() + Duration::from_secs(60) {
        session.refresh(http)?;
    }
    let mut apps = Vec::new();
    let mut page_token = String::new();
    let mut seen_pages = HashSet::new();
    let mut refreshed = false;
    loop {
        check_cancelled(cancelled)?;
        let response = http
            .get(endpoint)
            .bearer_auth(session.token.access_token().secret())
            .query(&[("pageSize", "1000"), ("pageToken", page_token.as_str())])
            .send()
            .map_err(|_| "Could not load your Play Store apps. Check your internet connection and retry.")?;
        if response.status() == StatusCode::UNAUTHORIZED && !refreshed {
            session.refresh(http)?;
            refreshed = true;
            continue;
        }
        if !response.status().is_success() {
            return Err(api_error(
                response.status(),
                response.json().unwrap_or_default(),
            ));
        }
        let page: AppPage = response
            .json()
            .map_err(|_| "Google Play returned an invalid app list. Please retry.")?;
        apps.extend(page.apps);
        if page.next_page_token.is_empty() {
            break;
        }
        if !seen_pages.insert(page.next_page_token.clone()) {
            return Err("Google Play returned a repeated page of apps. Please retry.".to_owned());
        }
        page_token = page.next_page_token;
    }
    apps.sort_by_cached_key(|app| (app.display_name.to_lowercase(), app.package_name.clone()));
    let mut packages = HashSet::new();
    apps.retain(|app| packages.insert(app.package_name.clone()));
    Ok(apps)
}

pub(super) fn api_error(status: StatusCode, body: serde_json::Value) -> String {
    let details = body["error"]["details"].as_array();
    if details.is_some_and(|details| {
        details
            .iter()
            .any(|detail| detail["reason"] == "SERVICE_DISABLED")
    }) {
        return "Enable the Google Play Developer Reporting API in the OAuth client's Google Cloud project, then click Refresh.".to_owned();
    }
    match status {
        StatusCode::UNAUTHORIZED => "Your Google Play session has expired. Disconnect and connect again.",
        StatusCode::FORBIDDEN => "Google Play denied access. Grant the reporting permission when connecting and check this account's Play Console app permissions.",
        StatusCode::TOO_MANY_REQUESTS => "Google Play is receiving too many requests. Wait a moment and refresh again.",
        _ => "Google Play could not load your apps. Please try refreshing again.",
    }.to_owned()
}
