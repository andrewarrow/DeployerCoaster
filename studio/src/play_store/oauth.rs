use super::*;

pub(super) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Google sign-in cancelled.".to_owned())
    } else {
        Ok(())
    }
}

pub(super) fn authorize(
    http: &Client,
    cancelled: &AtomicBool,
    send: &impl Fn(Event),
) -> Result<Session, String> {
    let credentials = Credentials::load()?;
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|_| "Could not start the local Google sign-in callback.")?;
    let port = listener
        .local_addr()
        .map_err(|_| "Could not get the sign-in callback port.")?
        .port();
    let redirect = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    let client = oauth_client(&credentials).set_redirect_uri(
        RedirectUrl::new(redirect).map_err(|_| "Invalid sign-in callback address.")?,
    );
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, state) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(PUBLISHER_SCOPE.to_owned()))
        .add_scope(Scope::new(REPORTING_SCOPE.to_owned()))
        .set_pkce_challenge(challenge)
        .add_extra_param("access_type", "offline")
        .add_extra_param("prompt", "consent select_account")
        .url();
    check_cancelled(cancelled)?;
    webbrowser::open(url.as_str()).map_err(|_| "Could not open your browser for Google sign-in. Check your default browser and try again.")?;
    send(Event::Progress("Finish signing in in your browser."));
    let code = wait_for_callback(&listener, &state, cancelled, SIGN_IN_TIMEOUT)?;
    check_cancelled(cancelled)?;
    send(Event::Progress("Completing Google sign-in…"));
    let token = client
        .exchange_code(code)
        .set_pkce_verifier(verifier)
        .request(http)
        .map_err(|_| "Could not complete Google sign-in. Check your connection and try again.")?;
    if let Some(scopes) = token.scopes()
        && [PUBLISHER_SCOPE, REPORTING_SCOPE]
            .iter()
            .any(|required| !scopes.iter().any(|scope| scope.as_str() == *required))
    {
        return Err("Google Play access was not fully granted. Connect again and allow both requested permissions.".to_owned());
    }
    Ok(Session::new(credentials, token))
}

pub(super) enum Callback {
    Code(AuthorizationCode),
    Denied,
}

pub(super) fn parse_callback(request: &str, expected_state: &CsrfToken) -> Result<Callback, ()> {
    let mut parts = request.split_whitespace();
    if parts.next() != Some("GET") {
        return Err(());
    }
    let target = parts.next().ok_or(())?;
    if !target.starts_with('/') || parts.next() != Some("HTTP/1.1") {
        return Err(());
    }
    let url = Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| ())?;
    if url.path() != CALLBACK_PATH {
        return Err(());
    }
    let mut state = None;
    let mut code = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if slot.replace(value.into_owned()).is_some() {
            return Err(());
        }
    }
    if CsrfToken::new(state.ok_or(())?) != *expected_state {
        return Err(());
    }
    match (code, error) {
        (Some(code), None) if !code.is_empty() => Ok(Callback::Code(AuthorizationCode::new(code))),
        (None, Some(_)) => Ok(Callback::Denied),
        _ => Err(()),
    }
}

pub(super) fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{message}",
        message.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

pub(super) fn wait_for_callback(
    listener: &TcpListener,
    state: &CsrfToken,
    cancelled: &AtomicBool,
    timeout: Duration,
) -> Result<AuthorizationCode, String> {
    listener
        .set_nonblocking(true)
        .map_err(|_| "Could not listen for Google sign-in.")?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        check_cancelled(cancelled)?;
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                let mut request = String::new();
                if BufReader::new((&mut stream).take(8192))
                    .read_line(&mut request)
                    .is_err()
                    || !request.ends_with('\n')
                {
                    respond(&mut stream, "400 Bad Request", "Invalid sign-in callback.");
                    continue;
                }
                match parse_callback(&request, state) {
                    Ok(Callback::Code(code)) => {
                        respond(
                            &mut stream,
                            "200 OK",
                            "Google authorization received. Return to DeployerCoaster to finish connecting.",
                        );
                        return Ok(code);
                    }
                    Ok(Callback::Denied) => {
                        respond(
                            &mut stream,
                            "200 OK",
                            "Google sign-in was cancelled. You can return to DeployerCoaster.",
                        );
                        return Err(
                            "Google sign-in was cancelled or denied. Connect again to retry."
                                .to_owned(),
                        );
                    }
                    Err(()) => respond(
                        &mut stream,
                        "400 Bad Request",
                        "Invalid sign-in callback. Continue signing in from DeployerCoaster.",
                    ),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                return Err(
                    "Could not receive the Google sign-in callback. Try connecting again."
                        .to_owned(),
                );
            }
        }
    }
    Err("Google sign-in timed out. Connect again to retry.".to_owned())
}
