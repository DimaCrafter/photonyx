use std::io::{BufRead, Error, ErrorKind, Read, Write};
use std::net::{SocketAddr, IpAddr, TcpStream, Shutdown};
use std::time::Instant;
use bufstream::BufStream;
use crate::app::config::{ServerConfig, CONFIG};
use crate::http::codes::HttpCode;
use crate::http::entity::{HttpConnection, HttpEngine, HttpMethod, ParsingResult, Request, Response, ResponseSink, ResponseType};

#[derive(Copy, Clone)]
pub struct Http1Engine;

impl HttpEngine<Http1Connection> for Http1Engine {
    fn handle_connection (socket: (TcpStream, SocketAddr)) -> Http1Connection {
        Http1Connection::new(socket, CONFIG.server)
    }
}

pub struct Http1Connection {
    stream: BufStream<TcpStream>,
    address: IpAddr,
    version_minor: char,
    limits: ServerConfig
}

impl Http1Connection {
    pub fn new (socket: (TcpStream, SocketAddr), limits: ServerConfig) -> Self {
        let _ = socket.0.set_write_timeout(Some(limits.write_timeout));

        Http1Connection {
            stream: BufStream::new(socket.0),
            address: socket.1.ip(),
            version_minor: '1',
            limits
        }
    }

    fn set_read_deadline (&self, deadline: Instant) -> Result<(), ParsingResult> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ParsingResult::Error(HttpCode::RequestTimeout));
        }

        return self.stream.get_ref().set_read_timeout(Some(remaining)).map_err(|_| ParsingResult::Invalid);
    }

    fn read_head (&mut self, deadline: Instant) -> Result<Vec<u8>, ParsingResult> {
        const END: &[u8] = b"\r\n\r\n";
        let mut head = Vec::new();

        loop {
            self.set_read_deadline(deadline)?;

            let chunk = match self.stream.fill_buf() {
                Ok([]) => return Err(ParsingResult::Invalid),
                Ok(chunk) => chunk,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(read_error(error))
            };

            let consumed = head.len();
            head.extend_from_slice(chunk);

            // The terminator can be split between reads
            let search_from = consumed.saturating_sub(END.len() - 1);
            let end = head[search_from..]
                .windows(END.len())
                .position(|part| part == END)
                .map(|i| search_from + i + END.len());

            let used = end.map_or(head.len(), |end| end) - consumed;
            self.stream.consume(used);

            if end.unwrap_or(head.len()) > self.limits.max_header_size {
                return Err(ParsingResult::Error(HttpCode::RequestHeaderFieldsTooLarge));
            }

            if let Some(end) = end {
                head.truncate(end);
                return Ok(head);
            }
        }
    }

    fn parse_head (&mut self, head: &[u8]) -> Result<Request, ParsingResult> {
        let mut lines = head.split(|byte| *byte == b'\n');

        let request_line = lines.next().and_then(|line| line.strip_suffix(b"\r")).ok_or(ParsingResult::Invalid)?;
        let parts: Vec<&[u8]> = request_line.split(|byte| *byte == b' ').collect();
        let [method, path, version] = parts.as_slice() else {
            return Err(ParsingResult::Invalid);
        };

        let [b'H', b'T', b'T', b'P', b'/', b'1', b'.', minor] = **version else {
            return Err(ParsingResult::Invalid);
        };

        self.version_minor = minor as char;

        let method = HttpMethod::from_str(&String::from_utf8_lossy(method)).ok_or(ParsingResult::Error(HttpCode::MethodNotAllowed))?;
        let mut req = Request::new(method, String::from_utf8_lossy(path).into_owned());

        for line in lines {
            let line = line.strip_suffix(b"\r").ok_or(ParsingResult::Error(HttpCode::BadRequest))?;
            if line.is_empty() {
                break;
            }

            let colon = line.iter().position(|byte| *byte == b':').filter(|index| *index != 0);
            let colon = colon.ok_or(ParsingResult::Error(HttpCode::BadRequest))?;

            let name = String::from_utf8_lossy(&line[..colon]).into_owned();
            let value = String::from_utf8_lossy(&line[(colon + 1)..]).into_owned();
            req.headers.set_normal(name, value);
        }

        return Ok(req);
    }

    fn read_body (&mut self, req: &mut Request, deadline: Instant) -> Result<(), ParsingResult> {
        if req.headers.get("transfer-encoding").is_some() {
            return Err(ParsingResult::Error(HttpCode::LengthRequired));
        }

        if req.headers.get("content-length").is_none() {
            return Ok(());
        }

        let len = req.parse_content_length().ok_or(ParsingResult::Error(HttpCode::BadRequest))?;
        if len > self.limits.max_body_size {
            return Err(ParsingResult::Error(HttpCode::RequestEntityTooLarge));
        }

        req.body.resize(len, 0);

        let mut filled = 0;
        while filled < len {
            self.set_read_deadline(deadline)?;

            match self.stream.read(&mut req.body[filled..]) {
                Ok(0) => return Err(ParsingResult::Invalid),
                Ok(count) => filled += count,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(read_error(error))
            }
        }

        return Ok(());
    }

    /// Writes status line and headers, the head stays unterminated
    fn write_status (&mut self, res: &Response) -> Result<(), Error> {
        let (res_code, res_reason) = res.code.get_description();
        write!(self.stream, "HTTP/1.{} {} {}", self.version_minor, res_code, res_reason)?;

        for header in &res.headers {
            write!(self.stream, "\r\n{}: {}", header.name, header.value)?;
        }

        return Ok(());
    }

    fn is_chunked (&self) -> bool {
        return self.version_minor != '0';
    }
}

fn read_error (error: Error) -> ParsingResult {
    return match error.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => ParsingResult::Error(HttpCode::RequestTimeout),
        _ => ParsingResult::Invalid
    };
}

impl ResponseSink for Http1Connection {
    fn begin_stream (&mut self, head: &Response) -> Result<(), Error> {
        self.write_status(head)?;

        if self.is_chunked() {
            self.stream.write_all(b"\r\nTransfer-Encoding: chunked")?;
        }

        self.stream.write_all(b"\r\nConnection: close\r\n\r\n")?;
        return self.stream.flush();
    }

    fn write_stream (&mut self, chunk: &[u8]) -> Result<(), Error> {
        if chunk.is_empty() {
            return Ok(());
        }

        if self.is_chunked() {
            write!(self.stream, "{:x}\r\n", chunk.len())?;
            self.stream.write_all(chunk)?;
            self.stream.write_all(b"\r\n")?;
        } else {
            self.stream.write_all(chunk)?;
        }

        return self.stream.flush();
    }

    fn finish_stream (&mut self) -> Result<(), Error> {
        if self.is_chunked() {
            self.stream.write_all(b"0\r\n\r\n")?;
        }

        return self.stream.flush();
    }
}

impl HttpConnection for Http1Connection {
    fn get_address (&self) -> IpAddr { self.address }

    fn into_stream (self) -> BufStream<TcpStream> {
        let _ = self.stream.get_ref().set_read_timeout(None);
        return self.stream;
    }

    fn parse (&mut self) -> ParsingResult {
        let deadline = Instant::now() + self.limits.read_timeout;

        let head = match self.read_head(deadline) {
            Ok(head) => head,
            Err(result) => return result
        };

        let mut req = match self.parse_head(&head) {
            Ok(req) => req,
            Err(result) => return result
        };

        if let HttpMethod::POST = req.method {
            if let Err(result) = self.read_body(&mut req, deadline) {
                return result;
            }
        }

        return ParsingResult::Complete(req);
    }

    fn respond (&mut self, res: Response) -> Result<(), Error> {
        match res.payload {
            ResponseType::Drop | ResponseType::Streamed => return Ok(()),
            ResponseType::Upgrade => {
                self.write_status(&res)?;
                self.stream.write_all(b"\r\n\r\n")?;
            }
            ResponseType::NoContent => {
                self.write_status(&res)?;
                self.stream.write_all(b"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
            }
            ResponseType::Payload(ref payload) => {
                self.write_status(&res)?;
                write!(self.stream, "\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len())?;
                self.stream.write_all(payload)?;
            }
        }

        return self.stream.flush();
    }

    fn disconnect (self) -> Result<(), Error> {
        let stream = self.stream.into_inner()?;
        return stream.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use std::{io::{Read, Write}, net::{TcpListener, TcpStream}, thread, time::{Duration, Instant}};
    use crate::http::{codes::HttpCode, entity::{HttpConnection, HttpHeaders, ParsingResult, Response, ResponseSink, ResponseType}};
    use crate::app::config::ServerConfig;
    use super::Http1Connection;

    fn limits () -> ServerConfig {
        return ServerConfig {
            max_header_size: 256,
            max_body_size: 1024,
            read_timeout: Duration::from_millis(200),
            write_timeout: Duration::from_millis(200)
        };
    }

    fn connect () -> (TcpStream, Http1Connection) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let server = Http1Connection::new(listener.accept().unwrap(), limits());
        return (client, server);
    }

    fn parse (input: &[u8]) -> ParsingResult {
        let (mut client, mut server) = connect();
        client.write_all(input).unwrap();
        return server.parse();
    }

    fn parse_error (input: &[u8]) -> Option<HttpCode> {
        return match parse(input) {
            ParsingResult::Error(code) => Some(code),
            _ => None
        };
    }

    fn read_all (mut client: TcpStream) -> String {
        let mut output = String::new();
        client.read_to_string(&mut output).unwrap();
        return output;
    }

    #[test]
    fn parses_request () {
        let ParsingResult::Complete(req) = parse(b"POST /a?b=1 HTTP/1.1\r\nContent-Length: 5\r\nX-Test:  value\r\n\r\nhello") else {
            panic!("expected complete request");
        };

        assert_eq!(req.path, "/a");
        assert_eq!(req.query, "b=1");
        assert_eq!(req.headers.get("x-test"), Some("value"));
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn body_is_optional () {
        let ParsingResult::Complete(req) = parse(b"POST / HTTP/1.1\r\n\r\n") else {
            panic!("expected complete request");
        };

        assert!(req.body.is_empty());
    }

    #[test]
    fn head_split_between_writes () {
        let (mut client, mut server) = connect();
        let writer = thread::spawn(move || {
            for part in [&b"GET / HTTP/1.1\r"[..], b"\nA: b\r\n\r", b"\n"] {
                client.write_all(part).unwrap();
                client.flush().unwrap();
                thread::sleep(Duration::from_millis(20));
            }
        });

        assert!(matches!(server.parse(), ParsingResult::Complete(_)));
        writer.join().unwrap();
    }

    #[test]
    fn rejects_body_over_limit_without_allocation () {
        let code = parse_error(b"POST / HTTP/1.1\r\nContent-Length: 99999999999999999\r\n\r\n");
        assert!(matches!(code, Some(HttpCode::RequestEntityTooLarge)));
    }

    #[test]
    fn rejects_invalid_content_length () {
        assert!(matches!(parse_error(b"POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n"), Some(HttpCode::BadRequest)));
    }

    #[test]
    fn rejects_chunked_request () {
        let code = parse_error(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n");
        assert!(matches!(code, Some(HttpCode::LengthRequired)));
    }

    #[test]
    fn rejects_head_over_limit () {
        let mut input = b"GET / HTTP/1.1\r\nX: ".to_vec();
        input.resize(4096, b'a');
        assert!(matches!(parse_error(&input), Some(HttpCode::RequestHeaderFieldsTooLarge)));
    }

    #[test]
    fn rejects_malformed_head () {
        assert!(matches!(parse(b"garbage\r\n\r\n"), ParsingResult::Invalid));
        assert!(matches!(parse_error(b"GET / HTTP/1.1\r\nno-colon\r\n\r\n"), Some(HttpCode::BadRequest)));
        assert!(matches!(parse_error(b"GET / HTTP/1.1\r\n: value\r\n\r\n"), Some(HttpCode::BadRequest)));
        assert!(matches!(parse_error(b"PUT / HTTP/1.1\r\n\r\n"), Some(HttpCode::MethodNotAllowed)));
    }

    #[test]
    fn silent_client_times_out () {
        let (_client, mut server) = connect();
        let start = Instant::now();

        assert!(matches!(server.parse(), ParsingResult::Error(HttpCode::RequestTimeout)));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn slow_client_is_limited_by_whole_request () {
        let (mut client, mut server) = connect();
        let writer = thread::spawn(move || {
            while client.write_all(b"a").is_ok() {
                thread::sleep(Duration::from_millis(10));
            }
        });

        let start = Instant::now();
        assert!(matches!(server.parse(), ParsingResult::Error(HttpCode::RequestTimeout)));
        assert!(start.elapsed() < Duration::from_secs(1));

        let _ = server.disconnect();
        writer.join().unwrap();
    }

    #[test]
    fn peer_closed_mid_head_does_not_panic () {
        let (mut client, mut server) = connect();
        client.write_all(b"GET / HT").unwrap();
        drop(client);

        assert!(matches!(server.parse(), ParsingResult::Invalid));
    }

    #[test]
    fn response_describes_payload () {
        let (client, mut server) = connect();
        let payload = vec![b'x'; 256 * 1024];

        let reader = thread::spawn(move || read_all(client));
        server.respond(Response {
            code: HttpCode::OK,
            headers: HttpHeaders::empty(),
            payload: ResponseType::Payload(payload)
        }).unwrap();
        server.disconnect().unwrap();

        let output = reader.join().unwrap();
        let (head, body) = output.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head.contains("Content-Length: 262144"));
        assert!(head.contains("Connection: close"));
        assert_eq!(body.len(), 256 * 1024);
    }

    #[test]
    fn error_before_version_is_valid_http () {
        let (client, mut server) = connect();
        server.respond(Response::from_status(HttpCode::RequestTimeout)).unwrap();
        server.disconnect().unwrap();

        assert!(read_all(client).starts_with("HTTP/1.1 408 "));
    }

    #[test]
    fn stream_http11_is_chunked () {
        let (mut client, mut server) = connect();
        client.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        assert!(matches!(server.parse(), ParsingResult::Complete(_)));

        let mut head = Response::empty();
        head.code = HttpCode::OK;
        server.begin_stream(&head).unwrap();
        server.write_stream(b"hello").unwrap();

        // the chunk has to be delivered while the handler is still running
        let mut received = [0; 256];
        client.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let count = client.read(&mut received).unwrap();
        let mut first = String::from_utf8_lossy(&received[..count]).into_owned();
        if !first.ends_with("hello\r\n") {
            let count = client.read(&mut received).unwrap();
            first += &String::from_utf8_lossy(&received[..count]);
        }

        assert!(first.contains("Transfer-Encoding: chunked"));
        assert!(first.ends_with("\r\n\r\n5\r\nhello\r\n"));

        server.write_stream(b"").unwrap();
        server.write_stream(b"0123456789abcdefg").unwrap();
        server.finish_stream().unwrap();
        server.disconnect().unwrap();

        assert_eq!(read_all(client), "11\r\n0123456789abcdefg\r\n0\r\n\r\n");
    }

    #[test]
    fn stream_http10_is_plain () {
        let (mut client, mut server) = connect();
        client.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
        assert!(matches!(server.parse(), ParsingResult::Complete(_)));

        let mut head = Response::empty();
        head.code = HttpCode::OK;
        server.begin_stream(&head).unwrap();
        server.write_stream(b"hello").unwrap();
        server.finish_stream().unwrap();
        server.disconnect().unwrap();

        let output = read_all(client);
        assert!(output.starts_with("HTTP/1.0 200 OK"));
        assert!(!output.contains("chunked"));
        assert!(output.ends_with("\r\n\r\nhello"));
    }

    #[test]
    fn stream_reports_gone_client () {
        let (client, mut server) = connect();

        let mut head = Response::empty();
        head.code = HttpCode::OK;
        server.begin_stream(&head).unwrap();
        drop(client);

        let failed = (0..50).any(|_| {
            thread::sleep(Duration::from_millis(5));
            return server.write_stream(b"line").is_err();
        });

        assert!(failed);
    }

    #[test]
    fn streamed_response_writes_nothing_more () {
        let (client, mut server) = connect();
        server.respond(Response {
            code: HttpCode::OK,
            headers: HttpHeaders::empty(),
            payload: ResponseType::Streamed
        }).unwrap();
        server.disconnect().unwrap();

        assert_eq!(read_all(client), "");
    }
}
