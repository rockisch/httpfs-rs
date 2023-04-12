use std::{
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
    time::Duration,
};

use clap::Parser;
use tokio::select;
use tokio_util::sync::CancellationToken;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value_t = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))]
    address: IpAddr,
    #[arg(short, long, default_value_t = 8000)]
    port: u16,
    #[arg(short, long, value_hint = clap::ValueHint::DirPath)]
    directory: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let root = args
        .directory
        .unwrap_or_else(|| PathBuf::from("."))
        .canonicalize()
        .unwrap();
    let address = format!("{}:{}", args.address, args.port);

    let cancel = CancellationToken::new();
    let cancel_sig = cancel.clone();
    let mut run_handle = tokio::spawn(async move {
        println!("http://{}", address);
        http_rust::run(&address, root, cancel_sig).await.unwrap()
    });

    select! {
        r = &mut run_handle => {
            r.unwrap();
            return;
        },
        _ = tokio::signal::ctrl_c() => {
            cancel.cancel();
        }
    };
    select! {
        r = &mut run_handle => {
            r.unwrap();
            return;
        }
        _ = tokio::time::sleep(Duration::from_secs(5)) => {
            panic!("forced shutdown after timeout");
        },
    };
}
