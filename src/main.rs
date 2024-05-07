use http_server::*;
use std::{net::TcpListener, process::exit};
fn main() {
    let args = Args::parse();

    let listener =
        TcpListener::bind((args.ip + ":" + args.port.as_str()).as_str()).unwrap_or_else(|err| {
            eprintln!("{err}");
            exit(1)
        });

    let pool = ThreadPool::new(args.threads as usize);

    for stream in listener.incoming() {
        let stream = stream.unwrap_or_else(|err| {
            eprintln!("{err}");
            exit(1)
        });

        pool.execute(|| {
            handle_connection(stream).unwrap_or_else(|err| {
                eprintln!("{err}");
                exit(1)
            })
        })
        .unwrap_or_else(|err| {
            eprintln!("{err}");
            exit(1)
        });
    }
}
