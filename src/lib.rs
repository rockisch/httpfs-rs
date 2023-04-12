#![feature(array_windows)]
#![feature(io_error_other)]
#![feature(lazy_cell)]
#![feature(try_blocks)]
mod http;

use std::io::Write as _;
use std::io::{self, Error};
use std::path::PathBuf;
use std::sync::Arc;

use mime_guess::Mime;
use tokio::fs::{read_dir, File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::select;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use http::{HttpHandler, HttpVersion, Method, ResponseOptions};

#[derive(Debug)]
struct State {
    root: PathBuf,
}

pub async fn run(address: &str, root: PathBuf, cancel: CancellationToken) -> io::Result<()> {
    let (sender, mut wg) = mpsc::channel::<()>(1);
    let listener = TcpListener::bind(address).await?;
    let state = Arc::new(State { root });
    select! {
        err = async {
            loop {
                let sender = sender.clone();
                let state = state.clone();
                let (stream, _) = listener.accept().await?;
                tokio::spawn(async move {
                    handle_stream(stream, &state).await;
                    drop(sender);
                });
            }
            #[allow(unreachable_code)]
            // We need this line or rust can't reason about the return value
            Ok::<_, io::Error>(())
        } => err?,
        _ = cancel.cancelled() => {},
    }
    drop(sender);
    // We want to use this as a 'WaitGroup', so ignore the error
    let _ = wg.recv().await;
    Ok(())
}

async fn handle_stream(stream: TcpStream, state: &State) {
    let mut handler = HttpHandler::new(stream);
    handle_request(&mut handler, state).await.unwrap();
    handler.stream.flush().await.unwrap();
}

async fn handle_request(handler: &mut HttpHandler, state: &State) -> io::Result<()> {
    let mut options = ResponseOptions::default();
    let Ok(request_line) = handler.read_request_line().await else {
        return handler.write_status("400 Bad Request", &options).await;
    };

    options.omit_body = match request_line.method {
        Method::Get => false,
        Method::Head => true,
        _ => return handler.write_status("405 Method Not Allowed", &options).await,
    };
    options.keep_open = match request_line.version {
        HttpVersion::Http1_0 => false,
        HttpVersion::Http1_1 => true,
        _ => return handler.write_status("505 HTTP Version Not Supported", &options).await,
    };
    match handle_path(handler, &request_line.uri, state, &options).await {
        Ok(r) => r,
        Err(_) => handler.write_status("500 Internal Server Error", &options).await,
    }
}

// Outer result is for internal errors, inner is for connection errors
async fn handle_path(
    handler: &mut HttpHandler,
    path_uri: &str,
    state: &State,
    options: &ResponseOptions,
) -> io::Result<io::Result<()>> {
    let Ok(path) = parse_path(&path_uri, &state.root).await else {
        return Ok(handler.write_status("404 Not Found", &options).await);
    };
    if path.is_dir() {
        let body = get_folder_body(path, path_uri).await?;
        Ok(handler.write_buffer("200 Ok", body, "text/html", options).await)
    } else {
        let (file, mime, len) = get_file_data(&path).await?;
        Ok(handler
            .write_reader("200 Ok", file, mime.essence_str(), len, options)
            .await)
    }
}

async fn parse_path(path_uri: &str, root: &PathBuf) -> io::Result<PathBuf> {
    let mut path = path_uri;
    if path.starts_with('/') {
        path = &path[1..];
    }
    let path = root.join(path).canonicalize()?;
    if !path.starts_with(root) {
        return Err(Error::other("invalid path"));
    }
    Ok(path)
}

async fn get_folder_body(dir: PathBuf, path_uri: &str) -> io::Result<Vec<u8>> {
    let mut rd = read_dir(dir).await?;
    let mut buf = Vec::with_capacity(1024);
    write!(
        buf,
        "<html><head><title>Directory listing for {0}</title><head><body><h1>Directory listing for {0}</h1><hr><ul>",
        path_uri
    )?;
    while let Some(d) = rd.next_entry().await? {
        let is_dir = d.file_type().await?.is_dir();
        write!(
            buf,
            "<li><a href=\"{0}{1}\">{0}</li>",
            d.file_name().to_str().unwrap(),
            if is_dir { "/" } else { "" }
        )?;
    }
    write!(buf, "</ul><hr></body></html>")?;
    Ok(buf)
}

async fn get_file_data(path: &PathBuf) -> io::Result<(File, Mime, usize)> {
    let file = OpenOptions::new().read(true).open(&path).await?;
    let meta = file.metadata().await?;
    let mime = mime_guess::from_path(path).first_or(mime_guess::mime::APPLICATION_OCTET_STREAM);
    Ok((file, mime, meta.len() as usize))
}
