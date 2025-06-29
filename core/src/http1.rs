use std::io::{Error, Read, Write};
use std::net::{SocketAddr, IpAddr, TcpStream, Shutdown};
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use bufstream::BufStream;
use photonyx_macro::assert_stream;
use crate::http::codes::HttpCode;
use crate::http::entity::{HttpConnection, HttpEngine, HttpMethod, ParsingResult, Request, Response, ResponseType};
use crate::utils::stream::StreamUtils;

#[derive(Copy, Clone)]
pub struct Http1Engine;

impl HttpEngine<Http1Connection> for Http1Engine {
    fn handle_connection (socket: (TcpStream, SocketAddr)) -> Http1Connection {
        Http1Connection::new(socket)
    }
}

pub struct Http1Connection {
    stream: BufStream<TcpStream>,
    address: IpAddr,
    version_minor: char
}

impl Http1Connection {
    fn new (socket: (TcpStream, SocketAddr)) -> Self {
        Http1Connection {
            stream: BufStream::new(socket.0),
            address: socket.1.ip(),
            version_minor: '\0'
        }
    }

    fn read_body (&mut self, body: &mut Vec<u8>) {
        let mut chunk = [0; 1024];

        loop {
            let n = self.stream.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }

            body.extend_from_slice(&chunk[..n]);
            if n < chunk.len() {
                break;
            }
        }
    }
}

impl HttpConnection for Http1Connection {
    fn get_address (&self) -> IpAddr { self.address }
    fn into_stream (self) -> BufStream<TcpStream> { self.stream }

    fn parse (&mut self) -> ParsingResult {
        let method = self.stream.read_string_before(' ');
        if method.is_none() { return ParsingResult::Invalid; }

        let method = HttpMethod::from_str(method.unwrap().as_str());
        if method.is_none() { return ParsingResult::Error(HttpCode::MethodNotAllowed) }

        let path = self.stream.read_string_before(' ');
        if path.is_none() { return ParsingResult::Error(HttpCode::RequestEntityTooLarge); }

        // let mut a = [0u8; 7];
        // self.stream.read_exact(&mut a);
        assert_stream!(self.stream, "HTTP/1.", ParsingResult::Invalid);
        self.version_minor = self.stream.read_u8().unwrap() as char;

        let mut req = Request::new(method.unwrap(), path.unwrap());

        assert_stream!(self.stream, "\r", ParsingResult::Invalid);
        loop {
            assert_stream!(self.stream, "\n", ParsingResult::Invalid);
            let mut header_name = Vec::new();
            header_name.push(self.stream.read_u8().unwrap());
            header_name.push(self.stream.read_u8().unwrap());

            if header_name[0] == '\r' as u8 && header_name[1] == '\n' as u8 {
                break;
            }

            let header_read_result = self.stream.read_before(':' as u8, &mut header_name);
            if header_read_result.is_none() { return ParsingResult::Error(HttpCode::RequestHeaderFieldsTooLarge) }

            let header_value = self.stream.read_string_before('\r');
            if header_value.is_none() { return ParsingResult::Error(HttpCode::RequestHeaderFieldsTooLarge) }

            let header_name = String::from_utf8_lossy(&header_name).into_owned();
            req.headers.set_normal(header_name, header_value.unwrap());
        }

        if let HttpMethod::POST = req.method {
            match req.parse_content_length() {
                Some(len) => {
                    req.body = Vec::with_capacity(len);
                    unsafe { req.body.set_len(len); }

                    self.stream.read_exact(req.body.as_mut_slice()).unwrap();
                }
                None => {
                    self.read_body(&mut req.body);
                }
            }
        }

        return ParsingResult::Complete(req);
    }

    fn respond (&mut self, res: Response) -> Result<(), Error> {
        if let ResponseType::Drop = res.payload {
            return Ok(());
        }

        self.stream.write(b"HTTP/1.")?;
        self.stream.write_u8(self.version_minor as u8)?;
        self.stream.write_u8(' ' as u8)?;
        let (res_code, res_reason) = res.code.get_description();

        self.stream.write(res_code.as_bytes())?;
        self.stream.write_u8(' ' as u8)?;
        self.stream.write(res_reason.as_bytes())?;

        for header in &res.headers {
            self.stream.write(b"\r\n")?;
            self.stream.write(header.name.as_bytes())?;
            self.stream.write(b": ")?;
            self.stream.write(header.value.as_bytes())?;
        }

        self.stream.write(b"\r\n\r\n")?;
        if let ResponseType::Payload(payload) = res.payload {
            self.stream.write(&payload)?;
        }

        return Ok(());
    }

    fn disconnect (self) -> Result<(), Error> {
        let stream = self.stream.into_inner()?;
        return stream.shutdown(Shutdown::Both);
    }
}
