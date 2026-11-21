use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use url::Url;

#[derive(Debug, PartialEq, Eq)]
pub struct AuthorizationCallback {
    pub code: String,
}

pub struct CallbackServer {
    listener: TcpListener,
    redirect_uri: String,
}

impl CallbackServer {
    pub fn bind() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("Failed to start callback listener: {e}"))?;
        let address = listener
            .local_addr()
            .map_err(|e| format!("Failed to get listener address: {e}"))?;
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{}/", address.port()),
        })
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn wait_for_callback(
        self,
        expected_state: &str,
        cancel_signal: Arc<AtomicBool>,
    ) -> Result<AuthorizationCallback, String> {
        self.listener
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to configure callback listener: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(300);

        loop {
            if cancel_signal.load(Ordering::Acquire) {
                return Err("Google authorization was cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("Google authorization timed out after 5 minutes".into());
            }
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .map_err(|e| format!("Failed to configure callback connection: {e}"))?;
                    stream
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .map_err(|e| format!("Failed to configure callback connection: {e}"))?;
                    if let Some(callback) = Self::handle_connection(&mut stream, expected_state)? {
                        return Ok(callback);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(format!("Failed to accept OAuth callback: {e}")),
            }
        }
    }

    fn handle_connection(
        stream: &mut TcpStream,
        expected_state: &str,
    ) -> Result<Option<AuthorizationCallback>, String> {
        let cloned = stream
            .try_clone()
            .map_err(|e| format!("Failed to read callback request: {e}"))?;
        let mut reader = BufReader::new(cloned);
        let mut request_line = String::new();
        reader
            .read_line(&mut request_line)
            .map_err(|e| format!("Failed to read HTTP request: {e}"))?;

        if request_line.len() > 8192 {
            Self::send_response(stream, "400 Bad Request", "Invalid request.")?;
            return Ok(None);
        }
        loop {
            let mut header = String::new();
            let bytes = reader
                .read_line(&mut header)
                .map_err(|e| format!("Failed to read HTTP headers: {e}"))?;
            if bytes == 0 || header == "\r\n" || header == "\n" {
                break;
            }
            if header.len() > 8192 {
                Self::send_response(stream, "400 Bad Request", "Invalid request headers.")?;
                return Ok(None);
            }
        }

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 || parts[0] != "GET" {
            Self::send_response(stream, "400 Bad Request", "Invalid callback request.")?;
            return Ok(None);
        }
        let request_url = Url::parse(&format!("http://127.0.0.1{}", parts[1]))
            .map_err(|e| format!("Invalid callback URL: {e}"))?;
        if request_url.path() != "/" {
            Self::send_response(stream, "404 Not Found", "Callback endpoint not found.")?;
            return Ok(None);
        }
        let params: HashMap<String, String> = request_url.query_pairs().into_owned().collect();
        let received_state = match params.get("state") {
            Some(value) => value,
            None => {
                Self::send_response(
                    stream,
                    "400 Bad Request",
                    "Missing OAuth state. You may close this window.",
                )?;
                return Ok(None);
            }
        };
        if received_state != expected_state {
            Self::send_response(
                stream,
                "400 Bad Request",
                "OAuth state validation failed. You may close this window.",
            )?;
            return Ok(None);
        }
        if let Some(error) = params.get("error") {
            Self::send_response(
                stream,
                "400 Bad Request",
                "Google authorization was not completed. You may close this window.",
            )?;
            return Err(format!("Google authorization failed: {error}"));
        }
        let code = match params.get("code").filter(|value| !value.is_empty()) {
            Some(value) => value.clone(),
            None => {
                Self::send_response(
                    stream,
                    "400 Bad Request",
                    "Authorization code is missing. You may close this window.",
                )?;
                return Err("OAuth callback is missing authorization code".into());
            }
        };
        Self::send_response(
            stream,
            "200 OK",
            "Google authorization received. You can close this window and return to Flip V2.",
        )?;
        Ok(Some(AuthorizationCallback { code }))
    }

    fn send_response(stream: &mut TcpStream, status: &str, message: &str) -> Result<(), String> {
        let body = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Flip V2</title></head><body><h2>{}</h2></body></html>",
            message
        );
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .map_err(|e| format!("Failed to send callback response: {e}"))?;
        stream
            .flush()
            .map_err(|e| format!("Failed to flush callback response: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpStream, thread};

    #[test]
    fn binds_to_available_local_port() {
        let server = CallbackServer::bind().unwrap();
        assert!(server.redirect_uri().starts_with("http://127.0.0.1:"));
        assert!(server.redirect_uri().ends_with('/'));
    }

    #[test]
    fn receives_valid_authorization_callback() {
        let server = CallbackServer::bind().unwrap();
        let address = server
            .redirect_uri()
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        let handle = thread::spawn(move || server.wait_for_callback("expected-state", Arc::new(AtomicBool::new(false))));
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .write_all(
                b"GET /?code=sample-code&state=expected-state HTTP/1.1\r\nHost: localhost\r\n\r\n",
            )
            .unwrap();
        let callback = handle.join().unwrap().unwrap();
        assert_eq!(callback.code, "sample-code");
    }

    #[test]
    fn rejects_wrong_state_and_keeps_listener_waiting() {
        let server = CallbackServer::bind().unwrap();
        let address = server
            .redirect_uri()
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        let handle = thread::spawn(move || server.wait_for_callback("expected-state", Arc::new(AtomicBool::new(false))));
        let mut bad = TcpStream::connect(&address).unwrap();
        bad.write_all(b"GET /?code=bad-code&state=wrong-state HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        drop(bad);
        let mut good = TcpStream::connect(&address).unwrap();
        good.write_all(
            b"GET /?code=good-code&state=expected-state HTTP/1.1\r\nHost: localhost\r\n\r\n",
        )
        .unwrap();
        let callback = handle.join().unwrap().unwrap();
        assert_eq!(callback.code, "good-code");
    }
}
