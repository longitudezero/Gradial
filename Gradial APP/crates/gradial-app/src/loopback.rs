use std::{
    io::{self, Read, Write},
    net::TcpStream,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use gradial_core::{MAX_PLUGIN_LOOPBACK_BODY_BYTES, PluginLoopbackMethod, PluginLoopbackRequest};

pub(crate) const LOOPBACK_OPERATION_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_RESPONSE_HEADER_BYTES: usize = 16 * 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoopbackTransportFailure {
    Interrupted,
    TimedOut,
    Connection,
    Protocol,
    ResponseTooLarge,
    HttpStatus,
}

pub(crate) fn execute(
    request: &PluginLoopbackRequest,
    accepted_at: Instant,
    cancellation: &AtomicBool,
    shutdown: &AtomicBool,
) -> Result<(), LoopbackTransportFailure> {
    let deadline = accepted_at.checked_add(LOOPBACK_OPERATION_TIMEOUT).unwrap_or(accepted_at);
    ensure_running(deadline, cancellation, shutdown)?;
    let mut stream = TcpStream::connect_timeout(
        &request.origin().socket_addr(),
        remaining(deadline, cancellation, shutdown)?,
    )
    .map_err(classify_io)?;
    stream.set_nodelay(true).map_err(classify_io)?;

    let header = request_header(request);
    write_all_bounded(&mut stream, header.as_bytes(), deadline, cancellation, shutdown)?;
    write_all_bounded(&mut stream, request.body(), deadline, cancellation, shutdown)?;
    let response = read_response(&mut stream, deadline, cancellation, shutdown)?;
    validate_response(&response)
}

fn request_header(request: &PluginLoopbackRequest) -> String {
    let authority = request.origin().as_str().trim_start_matches("http://");
    let mut header = format!(
        "{} {} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nAccept: */*\r\nContent-Length: {}\r\n",
        request.method().as_str(),
        request.path(),
        request.body().len()
    );
    if request.method() == PluginLoopbackMethod::Post {
        header.push_str("Content-Type: ");
        header.push_str(request.content_type().as_str());
        header.push_str("\r\n");
    }
    header.push_str("\r\n");
    header
}

fn write_all_bounded(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
    cancellation: &AtomicBool,
    shutdown: &AtomicBool,
) -> Result<(), LoopbackTransportFailure> {
    while !bytes.is_empty() {
        stream
            .set_write_timeout(Some(remaining(deadline, cancellation, shutdown)?))
            .map_err(classify_io)?;
        let written = stream.write(bytes).map_err(classify_io)?;
        if written == 0 {
            return Err(LoopbackTransportFailure::Connection);
        }
        bytes = &bytes[written..];
        ensure_running(deadline, cancellation, shutdown)?;
    }
    Ok(())
}

fn read_response(
    stream: &mut TcpStream,
    deadline: Instant,
    cancellation: &AtomicBool,
    shutdown: &AtomicBool,
) -> Result<Vec<u8>, LoopbackTransportFailure> {
    let maximum = MAX_RESPONSE_HEADER_BYTES + 4 + MAX_PLUGIN_LOOPBACK_BODY_BYTES;
    let mut response = Vec::with_capacity(4_096);
    let mut buffer = [0_u8; 4_096];
    loop {
        stream
            .set_read_timeout(Some(remaining(deadline, cancellation, shutdown)?))
            .map_err(classify_io)?;
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if response.len().saturating_add(read) > maximum {
                    return Err(LoopbackTransportFailure::ResponseTooLarge);
                }
                response.extend_from_slice(&buffer[..read]);
                if declared_response_total(&response)?.is_some_and(|total| response.len() >= total)
                {
                    break;
                }
            }
            Err(error) => return Err(classify_io(error)),
        }
        ensure_running(deadline, cancellation, shutdown)?;
    }
    Ok(response)
}

fn declared_response_total(response: &[u8]) -> Result<Option<usize>, LoopbackTransportFailure> {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        if response.len() > MAX_RESPONSE_HEADER_BYTES {
            return Err(LoopbackTransportFailure::ResponseTooLarge);
        }
        return Ok(None);
    };
    let header = std::str::from_utf8(&response[..header_end])
        .map_err(|_| LoopbackTransportFailure::Protocol)?;
    for line in header.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(LoopbackTransportFailure::Protocol);
        };
        if name.eq_ignore_ascii_case("content-length") {
            let length =
                value.trim().parse::<usize>().map_err(|_| LoopbackTransportFailure::Protocol)?;
            if length > MAX_PLUGIN_LOOPBACK_BODY_BYTES {
                return Err(LoopbackTransportFailure::ResponseTooLarge);
            }
            return Ok(Some(header_end + 4 + length));
        }
    }
    Ok(None)
}

fn validate_response(response: &[u8]) -> Result<(), LoopbackTransportFailure> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(LoopbackTransportFailure::Protocol)?;
    if header_end > MAX_RESPONSE_HEADER_BYTES {
        return Err(LoopbackTransportFailure::ResponseTooLarge);
    }
    let header = std::str::from_utf8(&response[..header_end])
        .map_err(|_| LoopbackTransportFailure::Protocol)?;
    let mut lines = header.split("\r\n");
    let status = lines.next().ok_or(LoopbackTransportFailure::Protocol)?;
    let mut status_parts = status.split_whitespace();
    if !matches!(status_parts.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return Err(LoopbackTransportFailure::Protocol);
    }
    let status_code = status_parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(LoopbackTransportFailure::Protocol)?;
    if !(200..300).contains(&status_code) {
        return Err(LoopbackTransportFailure::HttpStatus);
    }

    let mut content_length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(LoopbackTransportFailure::Protocol)?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") && !value.eq_ignore_ascii_case("identity")
        {
            return Err(LoopbackTransportFailure::Protocol);
        }
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value.parse::<usize>().map_err(|_| LoopbackTransportFailure::Protocol)?;
            if content_length.replace(parsed).is_some() {
                return Err(LoopbackTransportFailure::Protocol);
            }
        }
    }
    let body = &response[header_end + 4..];
    if body.len() > MAX_PLUGIN_LOOPBACK_BODY_BYTES
        || content_length
            .is_some_and(|length| length > MAX_PLUGIN_LOOPBACK_BODY_BYTES || length != body.len())
    {
        return Err(LoopbackTransportFailure::ResponseTooLarge);
    }
    Ok(())
}

fn remaining(
    deadline: Instant,
    cancellation: &AtomicBool,
    shutdown: &AtomicBool,
) -> Result<Duration, LoopbackTransportFailure> {
    ensure_running(deadline, cancellation, shutdown)?;
    Ok(deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1)))
}

fn ensure_running(
    deadline: Instant,
    cancellation: &AtomicBool,
    shutdown: &AtomicBool,
) -> Result<(), LoopbackTransportFailure> {
    if cancellation.load(Ordering::Acquire) || shutdown.load(Ordering::Acquire) {
        return Err(LoopbackTransportFailure::Interrupted);
    }
    if Instant::now() >= deadline {
        return Err(LoopbackTransportFailure::TimedOut);
    }
    Ok(())
}

fn classify_io(error: io::Error) -> LoopbackTransportFailure {
    let kind = error.kind();
    drop(error);
    if matches!(kind, io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) {
        LoopbackTransportFailure::TimedOut
    } else {
        LoopbackTransportFailure::Connection
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    use gradial_core::{PluginLoopbackContentType, PluginLoopbackOrigin};

    use super::*;

    fn request(port: u16, method: PluginLoopbackMethod, body: &[u8]) -> PluginLoopbackRequest {
        PluginLoopbackRequest::new(
            method,
            format!("http://127.0.0.1:{port}").parse::<PluginLoopbackOrigin>().expect("origin"),
            "/control?private=value",
            body.to_vec(),
            PluginLoopbackContentType::Json,
        )
        .expect("request")
    }

    fn read_complete_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut captured = Vec::new();
        let mut buffer = [0_u8; 2_048];
        loop {
            let read = stream.read(&mut buffer).expect("read");
            assert_ne!(read, 0, "request ended before its declared body");
            captured.extend_from_slice(&buffer[..read]);
            let Some(header_end) = captured.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let header = String::from_utf8_lossy(&captured[..header_end]);
            let content_length = header
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Content-Length: ")
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .expect("content length");
            if captured.len() >= header_end + 4 + content_length {
                return captured;
            }
        }
    }

    #[test]
    fn direct_loopback_transport_sends_bounded_http_without_following_redirects() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("address").port();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let server_captured = Arc::clone(&captured);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream.set_read_timeout(Some(Duration::from_secs(1))).expect("timeout");
            let request = read_complete_request(&mut stream);
            server_captured.lock().expect("captured").extend_from_slice(&request);
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("response");
        });

        let cancellation = AtomicBool::new(false);
        let shutdown = AtomicBool::new(false);
        execute(
            &request(port, PluginLoopbackMethod::Post, br#"{"position":42}"#),
            Instant::now(),
            &cancellation,
            &shutdown,
        )
        .expect("request succeeds");
        server.join().expect("server");

        let captured = captured.lock().expect("captured");
        let text = String::from_utf8_lossy(&captured);
        assert!(text.starts_with("POST /control?private=value HTTP/1.1\r\n"));
        assert!(text.contains(&format!("Host: 127.0.0.1:{port}\r\n")));
        assert!(text.contains("Content-Type: application/json\r\n"));
        assert!(!text.contains("Proxy-Authorization"));
    }

    #[test]
    fn redirect_timeout_and_oversized_response_fail_closed() {
        for (response, expected) in [
            (
                b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/\r\nContent-Length: 0\r\n\r\n"
                    .as_slice(),
                LoopbackTransportFailure::HttpStatus,
            ),
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 65537\r\n\r\n".as_slice(),
                LoopbackTransportFailure::ResponseTooLarge,
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
            let port = listener.local_addr().expect("address").port();
            let response = response.to_vec();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut buffer = [0_u8; 1_024];
                let _read = stream.read(&mut buffer).expect("read");
                stream.write_all(&response).expect("response");
            });
            let result = execute(
                &request(port, PluginLoopbackMethod::Get, &[]),
                Instant::now(),
                &AtomicBool::new(false),
                &AtomicBool::new(false),
            );
            assert_eq!(result, Err(expected));
            server.join().expect("server");
        }

        assert_eq!(
            execute(
                &request(9, PluginLoopbackMethod::Get, &[]),
                Instant::now().checked_sub(LOOPBACK_OPERATION_TIMEOUT).expect("past instant"),
                &AtomicBool::new(false),
                &AtomicBool::new(false),
            ),
            Err(LoopbackTransportFailure::TimedOut)
        );
    }

    #[test]
    fn local_service_stall_cannot_exceed_the_transport_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("address").port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _request = read_complete_request(&mut stream);
            thread::sleep(Duration::from_millis(500));
            let _result = stream.write_all(
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        });
        let started = Instant::now();
        assert_eq!(
            execute(
                &request(port, PluginLoopbackMethod::Get, &[]),
                started,
                &AtomicBool::new(false),
                &AtomicBool::new(false),
            ),
            Err(LoopbackTransportFailure::TimedOut)
        );
        assert!(started.elapsed() < Duration::from_millis(450));
        server.join().expect("server");
    }
}
