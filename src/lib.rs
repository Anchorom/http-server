pub use clap::Parser;
use regex::Regex;
use serde_json::json;
use std::{
    collections::HashMap,
    error::Error,
    fs,
    io::prelude::*,
    net::TcpStream,
    net::ToSocketAddrs,
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
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

struct Worker {
    _id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(_id: usize, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv();

            match message {
                Ok(job) => {
                    job();
                }
                Err(_) => {
                    break;
                }
            }
        });

        Worker {
            _id,
            thread: Some(thread),
        }
    }
}

pub fn handle_connection(mut stream: TcpStream) -> Result<(), Box<dyn Error>> {
    let mut buffer = [0; 1024];
    stream.read(&mut buffer)?;

    let (method, path, _) = parse_request(&buffer)?;

    let (status_line, content_type, contents) = if method != "GET" || method != "POST" {
        let contents = fs::read_to_string("static/501.html")?;
        ("HTTP/1.1 501 OK", "text/html", contents)
    } else {
        match (method.as_str(), path.as_str()) {
            ("GET", "/") | ("GET", "/index.html") => read_static_file("static/index.html")?,
            ("GET", "/501.html") => {
                let contents = fs::read_to_string("static/501.html")?;
                (
                    "HTTP/1.1 501 Not Implemented",
                    detect_content_type("static/404.html"),
                    contents,
                )
            }
            ("GET", "/api/check") => read_static_file("data/data.txt")?,
            ("GET", "/api/list") => read_static_file("data/data.json")?,
            ("POST", "/api/echo") => handle_echo_request(&buffer)?,
            ("POST", "/api/upload") => handle_upload_request(&buffer)?,
            ("GET", path) if path.starts_with("/api/search") => handle_search_request(path)?,
            ("GET", path) if path.ends_with(".html") => {
                read_static_file(&format!("static{}", path))?
            }
            ("GET", path) if path.ends_with(".js") => read_static_file(&format!("static{}", path))?,
            ("GET", path) if path.ends_with(".json") => {
                read_static_file(&format!("static{}", path))?
            }
            ("GET", path) if path.ends_with(".css") => {
                read_static_file(&format!("static{}", path))?
            }
            _ => {
                let contents = fs::read_to_string("static/404.html")?;
                (
                    "HTTP/1.1 404 NOT FOUND",
                    detect_content_type("static/404.html"),
                    contents,
                )
            }
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

    let method = parts.next().ok_or("Invalid method")?.to_string();
    let path = parts.next().ok_or("Invalid path")?.to_string();
    let protocol = parts.next().ok_or("Invalid protocol")?.to_string();

    Ok((method, path, protocol))
}

fn read_static_file(path: &str) -> Result<(&'static str, &'static str, String), Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;
    Ok(("HTTP/1.1 200 OK", detect_content_type(path), contents))
}

fn detect_content_type(path: &str) -> &'static str {
    match path {
        _ if path.ends_with(".html") => "text/html",
        _ if path.ends_with(".js") => "text/javascript",
        _ if path.ends_with(".json") => "application/json",
        _ if path.ends_with(".css") => "text/css",
        _ => "text/plain",
    }
}

fn handle_echo_request(
    buffer: &[u8],
) -> Result<(&'static str, &'static str, String), Box<dyn Error>> {
    let body_string: String = String::from_utf8_lossy(buffer).to_string();
    let body: Vec<&str> = body_string.trim_matches('\0').split("\r\n\r\n").collect();

    let data = if let Some(body) = body.get(1) {
        *body
    } else {
        return Err("No body found in the request".into());
    };

    let re = Regex::new("id=[0-9]+&name=[a-zA-Z0-9]+")?;

    match re.is_match(data) {
        true => Ok((
            "HTTP/1.1 200 OK",
            "application/x-www-form-urlencoded",
            format!("{data}"),
        )),
        false => Ok((
            "HTTP/1.1 403 Data format error",
            "text/plain",
            fs::read_to_string("data/error.txt")?,
        )),
    }
}

fn handle_upload_request(
    buffer: &[u8],
) -> Result<(&'static str, &'static str, String), Box<dyn Error>> {
    let content_type = extract_content_type(&buffer)?;

    let content_type = content_type.as_str();

    match content_type {
        "application/json" => {
            let body_string: String = String::from_utf8_lossy(&buffer).to_string();
            let body: Vec<&str> = body_string.trim_matches('\0').split("\r\n\r\n").collect();
            let data = if let Some(body) = body.get(1) {
                *body
            } else {
                return Err("No body found in the request".into());
            };

            Ok(("HTTP/1.1 200 OK", "application/json", data.to_string()))
        }
        "application/x-www-form-urlencoded" => {
            let body_string: String = String::from_utf8_lossy(&buffer).to_string();
            let body_parts: Vec<&str> = body_string.trim_matches('\0').split("\r\n\r\n").collect();

            let data = if let Some(body) = body_parts.get(1) {
                *body
            } else {
                return Err("No body found in the request".into());
            };

            let re = Regex::new(r"id=\d+&name=[a-zA-Z0-9]+")?;

            if let Some(captures) = re.captures(data) {
                let id = captures.get(1).map_or("", |m| m.as_str());
                let name = captures.get(2).map_or("", |m| m.as_str());

                let response = json!({
                    "id": id,
                    "name": name
                });

                Ok(("HTTP/1.1 200 OK", "application/json", response.to_string()))
            } else {
                let response = fs::read_to_string("data/error.json")?;
                Ok((
                    "HTTP/1.1 403 Data format error",
                    "application/json",
                    response,
                ))
            }
        }

        _ => Ok((
            "HTTP/1.1 404 NOT FOUND",
            "text/html",
            fs::read_to_string("static/404.html")?,
        )),
    }
}

fn handle_search_request(
    path: &str,
) -> Result<(&'static str, &'static str, String), Box<dyn Error>> {
    let path_parts: Vec<&str> = path.split('?').collect();

    let query_params = if path_parts.len() == 2 {
        parse_query_params(path_parts[1])?
    } else {
        HashMap::new()
    };

    let id = query_params.get("id").map_or("", |id| id.trim());
    let name = query_params.get("name").map_or("", |name| name.trim());

    let data = fs::read_to_string("data/data.json")?;
    let json_data: serde_json::Value = serde_json::from_str(&data)?;

    let matching_objects: Vec<_> = json_data
        .as_array()
        .ok_or("JSON data is not an array")?
        .iter()
        .filter(|obj| {
            obj.get("id")
                .and_then(|id_val| id_val.as_u64())
                .map_or(false, |id_val| id_val == id.parse::<u64>().unwrap())
                && obj
                    .get("name")
                    .and_then(|name_val| name_val.as_str())
                    .map_or(false, |name_val| name_val == name)
        })
        .collect();

    if !matching_objects.is_empty() {
        let response = serde_json::to_string(&matching_objects)?;
        Ok(("HTTP/1.1 200 OK", "application/json", response))
    } else {
        let response = fs::read_to_string("data/not_found.json")?;
        Ok(("HTTP/1.1 404 NOT FOUND", "application/json", response))
    }
}

fn extract_content_type(buffer: &[u8]) -> Result<String, Box<dyn Error>> {
    let request = String::from_utf8_lossy(buffer);
    let headers: Vec<&str> = request.trim_matches('\0').split("\r\n").collect();

    let content_type_line = headers.get(4).ok_or("No Content-Type header found")?;
    let content_type_parts: Vec<&str> = content_type_line.split(":").collect();

    if content_type_parts.len() != 2 || content_type_parts[0].trim() != "Content-Type" {
        return Err("Invalid Content-Type header".into());
    }

    Ok(content_type_parts[1].trim().to_string())
}

fn parse_query_params(query: &str) -> Result<HashMap<String, String>, String> {
    let mut params = HashMap::new();

    for param in query.split('&') {
        let parts: Vec<&str> = param.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err("Invalid query parameter format".to_string());
        }
        params.insert(parts[0].to_string(), parts[1].to_string());
    }

    Ok(params)
}

fn _proxy_request(
    mut _client_stream: TcpStream,
    _proxy_address: String,
) -> Result<(), Box<dyn Error>> {
    Ok(())
}

pub fn extract_proxy_address(proxy: &str) -> Result<String, Box<dyn Error>> {
    let re = Regex::new(r"http://([^:/]+):?(\d+)?").unwrap();

    if let Some(captures) = re.captures(proxy) {
        let address = captures.get(1).unwrap().as_str();
        let port_str = captures.get(2).map_or("80", |port| port.as_str());

        let port: u16 = match port_str.parse() {
            Ok(port) => port,
            Err(_) => return Err("Failed to parse port".into()),
        };

        let addr = match (address, port).to_socket_addrs() {
            Ok(mut addr_iter) => match addr_iter.next() {
                Some(addr) => addr,
                None => return Err("Failed to resolve address".into()),
            },
            Err(e) => return Err(format!("Socket address resolution error: {}", e).into()),
        };

        Ok(addr.to_string())
    } else {
        Err("Invalid proxy address format".into())
    }
}
