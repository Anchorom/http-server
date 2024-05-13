use http_server::*;
use std::{net::TcpListener, process::exit};

fn main() {
    let args = Args::parse();

    let listener = TcpListener::bind(format!("{}:{}", args.ip, args.port))
        .unwrap_or_else(|err| exit_with_error(&format!("{}", err)));

    let pool = ThreadPool::new(args.threads as usize);

    //let proxy_enabled = !args.proxy.is_empty();

    // let proxy_address = if proxy_enabled {
    //     match extract_proxy_address(&args.proxy) {
    //         Ok(proxy_address) => proxy_address,
    //         Err(err) => exit_with_error(&format!("{}", err)),
    //     }
    // } else {
    //     String::new()
    // };

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                //let proxy_address_clone = proxy_address.clone();

                pool.execute(move || {
                    handle_connection(stream).unwrap_or_else(|err| {
                        exit_with_error(&format!("{}", err));
                    })
                })
                .unwrap_or_else(|err| {
                    exit_with_error(&format!("{}", err));
                });
            }
            Err(err) => {
                eprintln!("{}", err);
            }
        }
    }
}

fn exit_with_error(msg: &str) -> ! {
    eprintln!("{}", msg);
    exit(1);
}
