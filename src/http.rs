use std::collections::HashMap;
use std::io::Write as _;
use std::io::{self, Error};
use std::sync::LazyLock;
use std::time::SystemTime;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Clone, Copy, Debug)]
pub enum Method {
    Get,
    Post,
    Head,
    Put,
    Delete,
    Connect,
    Options,
    Trace,
    Patch,
}

#[derive(Debug)]
pub enum HttpVersion {
    Http1_0,
    Http1_1,
    Unknown,
}

#[derive(Debug)]
pub struct RequestLine {
    pub method: Method,
    pub uri: String,
    pub version: HttpVersion,
}

#[derive(Debug, Default)]
pub struct ResponseOptions {
    pub keep_open: bool,
    pub omit_body: bool,
}

static METHODS_HASH: LazyLock<HashMap<&'static [u8], Method>> = LazyLock::new(|| {
    HashMap::from([
        (b"GET" as &[u8], Method::Get),
        (b"HEAD", Method::Head),
        (b"POST", Method::Post),
        (b"PUT", Method::Put),
        (b"DELETE", Method::Delete),
        (b"CONNECT", Method::Connect),
        (b"OPTIONS", Method::Options),
        (b"TRACE", Method::Trace),
        (b"PATCH", Method::Patch),
    ])
});

#[derive(Debug)]
pub struct HttpHandler {
    pub stream: TcpStream,
    buf: Vec<u8>,
}

impl HttpHandler {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buf: Vec::with_capacity(1024),
        }
    }

    pub async fn read_request_line(&mut self) -> io::Result<RequestLine> {
        let mut cursor = 0;
        let reqline_end = loop {
            let n = self.stream.read_buf(&mut self.buf).await?;
            if n == 0 {
                return Err(Error::other("connection closed"));
            }
            if let Some(i) = self.buf[cursor..].array_windows::<2>().position(|v| v == b"\r\n") {
                break cursor + i;
            }
            // If we didn't find the CRLF, we don't need to re-scan the entire buffer next time
            cursor = self.buf.len() - 1;
        };

        let mut parts = self.buf[..reqline_end].split(|&v| v == b' ');
        let &method = parts
            .next()
            .and_then(|v| METHODS_HASH.get(v))
            .ok_or(Error::other("invalid request line"))?;
        let uri = parts
            .next()
            .and_then(|v| std::str::from_utf8(v).ok())
            .ok_or(Error::other("invalid request line"))?
            .to_string();
        let version = parts
            .next()
            .map(|v| match v {
                b"HTTP/1.0" => HttpVersion::Http1_0,
                b"HTTP/1.1" => HttpVersion::Http1_1,
                _ => HttpVersion::Unknown,
            })
            .ok_or(Error::other("invalid request line"))?;
        Ok(RequestLine { method, uri, version })
    }

    fn prepare_response_body(&mut self, status: &str, ctype: &str, clen: usize) {
        self.buf.clear();
        let date_header = httpdate::fmt_http_date(SystemTime::now());
        write!(&mut self.buf, "HTTP/1.0 {}\r\n", status).unwrap();
        write!(&mut self.buf, "Date: {}\r\n", date_header).unwrap();
        write!(&mut self.buf, "Content-Type: {}\r\n", ctype).unwrap();
        write!(&mut self.buf, "Content-Length: {}\r\n", clen).unwrap();
        write!(&mut self.buf, "\r\n").unwrap();
    }

    pub async fn write_status(&mut self, status: &str, options: &ResponseOptions) -> io::Result<()> {
        self.prepare_response_body(status, "text", status.len());
        if !options.omit_body {
            write!(&mut self.buf, "{}", status)?;
            self.stream.write_all(&mut self.buf).await?;
        }
        Ok(())
    }

    pub async fn write_buffer(
        &mut self,
        status: &str,
        mut buf: Vec<u8>,
        ctype: &str,
        options: &ResponseOptions,
    ) -> io::Result<()> {
        self.prepare_response_body(status, ctype, buf.len());
        if !options.omit_body {
            self.stream.write_all(&mut self.buf).await?;
            self.stream.write_all(&mut buf).await?;
        }
        Ok(())
    }

    pub async fn write_reader<B>(
        &mut self,
        status: &str,
        mut cbody: B,
        ctype: &str,
        clen: usize,
        options: &ResponseOptions,
    ) -> io::Result<()>
    where
        B: AsyncRead + Unpin,
    {
        self.prepare_response_body(status, ctype, clen);
        if !options.omit_body {
            self.stream.write_all(&mut self.buf).await?;
            tokio::io::copy(&mut cbody, &mut self.stream).await?;
        }
        Ok(())
    }
}
