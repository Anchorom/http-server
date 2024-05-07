use http_server::*;
use std::{net::TcpListener, process::exit};

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap_or_else(|err| {
        eprintln!("{err}");
        exit(1)
    });

    let pool = ThreadPool::new(8);

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
