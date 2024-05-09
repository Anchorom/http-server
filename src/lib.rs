pub use clap::Parser;
use regex::Regex;
use std::{
    error::Error,
    fs,
    io::prelude::*,
    net::TcpStream,
    sync::{mpsc, Arc, Mutex},
    thread,
};

#[derive(Parser, Debug)]
pub struct Args {
    /// IP
    #[arg(short, long, default_value_t = String::from("127.0.0.1"))]
    pub ip: String,

    /// 端口
    #[arg(short, long, default_value_t = String::from("8080"))]
    pub port: String,

    ///线程数
    #[arg(short, long, default_value_t = 8_u8)]
    pub threads: u8,

    ///代理
    #[arg(long, default_value_t = String::from(""))]
    pub proxy: String,
}

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<mpsc::Sender<Job>>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

impl ThreadPool {
    /// Create a new ThreadPool.
    ///
    /// The size is the number of threads in the pool.
    ///
    /// # Panics
    ///
    /// The `new` function will panic if the size is zero.
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();

        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }

        ThreadPool {
            workers,
            sender: Some(sender),
        }
    }

    pub fn execute<F>(&self, f: F) -> Result<(), Box<dyn Error>>
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);

        self.sender.as_ref().unwrap().send(job)?;
        Ok(())
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(self.sender.take());

        for worker in &mut self.workers {
            println!("Shutting down worker {}", worker.id);

            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv();

            match message {
                Ok(job) => {
                    println!("Worker {id} got a job; executing.");

                    job();
                }
                Err(_) => {
                    println!("Worker {id} disconnected; shutting down.");
                    break;
                }
            }
        });

        Worker {
            id,
            thread: Some(thread),
        }
    }
}

pub fn handle_connection(mut stream: TcpStream) -> Result<(), Box<dyn Error>> {
    let mut buffer = [0; 1024];
    stream.read(&mut buffer)?;

    let re = Regex::new("id=[0-9]+&name=[a-zA-Z0-9]+")?;

    let (method, path, _) = parse_request(&buffer)?;

    let (status_line, content_type, contents) = match (method.as_str(), path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => {
            let contents = fs::read_to_string("static/index.html")?;
            ("HTTP/1.1 200 OK", "text/html", contents)
        }
        ("GET", "/501.html") => {
            let contents = fs::read_to_string("static/501.html")?;
            ("HTTP/1.1 501 OK", "text/html", contents)
        }
        ("GET", "/api/check") => {
            let contents = fs::read_to_string("data/data.txt")?;
            ("HTTP/1.1 200 OK", "text/html", contents)
        }
        ("POST", "/api/echo") => {
            let body_string = String::from_utf8_lossy(&buffer).to_string();
            let body: Vec<&str> = body_string.split("\r\n\r\n").collect();

            let data = if let Some(body) = body.get(1) {
                *body
            } else {
                return Err("No body found in the request".into());
            };
            match re.is_match(data) {
                true => {
                    let contents = format!("{}", "id=1&name=Foo");
                    (
                        "HTTP/1.1 200 OK",
                        "application/x-www-form-urlencoded",
                        contents,
                    )
                }
                false => (
                    "HTTP/1.1 404 OK",
                    "text/plain",
                    fs::read_to_string("data/error.txt")?,
                ),
            }
        }
        _ => {
            let contents = fs::read_to_string("static/404.html")?;
            ("HTTP/1.1 404 NOT FOUND", "text/html", contents)
        }
    };

    let response = format!(
        "{}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n{}",
        status_line,
        content_type,
        contents.len(),
        contents
    );

    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn parse_request(buffer: &[u8]) -> Result<(String, String, String), Box<dyn Error>> {
    let request = String::from_utf8_lossy(buffer);
    let mut lines = request.lines();

    let first_line = lines.next().ok_or("Empty request")?;
    let mut parts = first_line.split_whitespace();

    let method = parts.next().ok_or("Invalid request")?.to_string();
    let path = parts.next().ok_or("Invalid request")?.to_string();
    let protocol = parts.next().ok_or("Invalid request")?.to_string();

    Ok((method, path, protocol))
}
