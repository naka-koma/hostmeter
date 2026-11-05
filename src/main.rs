mod api;
mod model;
mod nvidia;
mod procmem;
mod store;

use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use model::{Adapter, Sample};
use store::Store;

const USAGE: &str = "hostmeter - samples the host's GPU load and serves it as JSON (docs/contract.md)

Usage: hostmeter [--listen ADDR] [--interval DURATION] [--retention DURATION]

  --listen     address to listen on            (default 127.0.0.1:8830)
  --interval   time between samples            (default 2s)
  --retention  how much history is kept        (default 30m)

A duration is a whole number with a unit: 500ms, 2s, 30m, 1h.";

struct Config {
    listen: String,
    interval: Duration,
    retention: Duration,
}

/// `500ms`, `2s`, `30m`, `1h`.
fn parse_duration(text: &str) -> Option<Duration> {
    let digits = text.chars().take_while(|c| c.is_ascii_digit()).count();
    let (number, unit) = text.split_at(digits);
    let number: u64 = number.parse().ok()?;
    let millis = match unit {
        "ms" => number,
        "s" => number.checked_mul(1000)?,
        "m" => number.checked_mul(60_000)?,
        "h" => number.checked_mul(3_600_000)?,
        _ => return None,
    };
    (millis > 0).then(|| Duration::from_millis(millis))
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut config = Config { listen: "127.0.0.1:8830".into(), interval: Duration::from_secs(2), retention: Duration::from_secs(30 * 60) };
    let mut iterator = args.iter();
    while let Some(flag) = iterator.next() {
        let mut value = |name: &str| iterator.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match flag.as_str() {
            "--listen" => config.listen = value("--listen")?,
            "--interval" => {
                let text = value("--interval")?;
                config.interval = parse_duration(&text).ok_or_else(|| format!("not a duration: {text}"))?;
            }
            "--retention" => {
                let text = value("--retention")?;
                config.retention = parse_duration(&text).ok_or_else(|| format!("not a duration: {text}"))?;
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }
    if config.retention < config.interval {
        return Err("--retention must be at least --interval".into());
    }
    Ok(config)
}

/// One reading: the driver's adapters, with the per-process list attached to the first one (version 1 reports one list).
fn take_sample() -> Sample {
    let mut adapters: Vec<Adapter> = nvidia::read();
    if let Some(first) = adapters.first_mut() {
        first.processes = procmem::read();
    }
    Sample { at: time::OffsetDateTime::now_utc(), adapters }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("hostmeter {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let config = match parse_args(&args) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("hostmeter: {message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let server = match tiny_http::Server::http(&config.listen) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("hostmeter: cannot listen on {}: {error}", config.listen);
            std::process::exit(1);
        }
    };
    let store = Arc::new(RwLock::new(Store::new(config.interval, config.retention)));

    // The sampler: one reading every interval, however long a reading takes (a slow one is not followed by a burst).
    {
        let store = Arc::clone(&store);
        let interval = config.interval;
        thread::spawn(move || loop {
            let started = Instant::now();
            let sample = take_sample();
            if let Ok(mut store) = store.write() {
                store.push(sample);
            }
            if let Some(rest) = interval.checked_sub(started.elapsed()) {
                thread::sleep(rest);
            }
        });
    }

    let started = Instant::now();
    eprintln!("hostmeter {} listening on http://{} (every {:?}, keeping {:?})", env!("CARGO_PKG_VERSION"), config.listen, config.interval, config.retention);
    let json: tiny_http::Header = "Content-Type: application/json; charset=utf-8".parse().expect("a valid header");
    for request in server.incoming_requests() {
        let settings = api::Settings { interval: config.interval, retention: config.retention, uptime_seconds: started.elapsed().as_secs() };
        let (status, body) = match store.read() {
            Ok(store) => api::route(request.method().as_str(), request.url(), &store, &settings, time::OffsetDateTime::now_utc()),
            Err(_) => (500, "{\"error\":\"internal error\"}".to_string()),
        };
        let response = tiny_http::Response::from_string(body).with_status_code(status).with_header(json.clone());
        let _ = request.respond(response);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration("2s"), Some(Duration::from_secs(2)));
        assert_eq!(parse_duration("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        for bad in ["", "s", "0s", "2", "2x", "-2s", "1.5s"] {
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }

    #[test]
    fn arguments() {
        let none = parse_args(&[]).unwrap();
        assert_eq!((none.listen.as_str(), none.interval, none.retention), ("127.0.0.1:8830", Duration::from_secs(2), Duration::from_secs(1800)));
        let given: Vec<String> = ["--listen", "0.0.0.0:9", "--interval", "1s", "--retention", "5m"].iter().map(|s| s.to_string()).collect();
        let config = parse_args(&given).unwrap();
        assert_eq!((config.listen.as_str(), config.interval, config.retention), ("0.0.0.0:9", Duration::from_secs(1), Duration::from_secs(300)));
        assert!(parse_args(&["--nope".to_string()]).is_err());
        assert!(parse_args(&["--interval".to_string()]).is_err());
        assert!(parse_args(&["--interval".into(), "10s".into(), "--retention".into(), "5s".into()]).is_err());
    }
}
