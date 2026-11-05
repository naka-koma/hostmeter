//! The routes of the HTTP contract (docs/contract.md). Pure: a request and the store in, a status and a body out.

use std::time::Duration;

use serde_json::json;
use time::OffsetDateTime;

use crate::model::{rfc3339, History, HistoryAdapter, HistorySample, Latest};
use crate::store::Store;

pub const SCHEMA: u32 = 1;

pub struct Settings {
    pub interval: Duration,
    pub retention: Duration,
    pub uptime_seconds: u64,
}

fn error(status: u16, message: &str) -> (u16, String) {
    (status, json!({ "error": message }).to_string())
}

/// `seconds=N` of the history query: a positive whole number, capped at the retention. `None` is the whole retention.
fn seconds_of(query: &str, retention: u64) -> Result<u64, ()> {
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("seconds=") {
            return match value.parse::<u64>() {
                Ok(seconds) if seconds > 0 => Ok(seconds.min(retention)),
                _ => Err(()),
            };
        }
    }
    Ok(retention)
}

pub fn route(method: &str, url: &str, store: &Store, settings: &Settings, now: OffsetDateTime) -> (u16, String) {
    if method != "GET" {
        return error(404, "not found");
    }
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let interval_ms = settings.interval.as_millis() as u64;
    match path.trim_end_matches('/') {
        "/health" => (
            200,
            json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION"), "uptime_seconds": settings.uptime_seconds }).to_string(),
        ),
        "/v1/gpu" => {
            let (sampled_at, adapters, available) = match store.latest() {
                Some(sample) => (sample.at, sample.adapters.as_slice(), !sample.adapters.is_empty()),
                None => (now, &[][..], false),
            };
            let body = Latest { schema: SCHEMA, sampled_at: rfc3339(sampled_at), interval_ms, available, adapters };
            (200, serde_json::to_string(&body).unwrap_or_default())
        }
        "/v1/gpu/history" => {
            let retention = settings.retention.as_secs();
            let Ok(seconds) = seconds_of(query, retention) else {
                return error(400, "seconds must be a positive whole number");
            };
            let since = now - time::Duration::seconds(seconds as i64);
            let samples = store
                .since(since)
                .into_iter()
                .map(|sample| HistorySample {
                    t: rfc3339(sample.at),
                    adapters: sample
                        .adapters
                        .iter()
                        .map(|adapter| HistoryAdapter {
                            index: adapter.index,
                            utilization_percent: adapter.utilization_percent,
                            memory_used_mib: adapter.memory.used_mib,
                            temperature_c: adapter.temperature_c,
                            power_watts: adapter.power_watts,
                            processes: &adapter.processes,
                        })
                        .collect(),
                })
                .collect();
            let body = History { schema: SCHEMA, interval_ms, retention_seconds: retention, samples };
            (200, serde_json::to_string(&body).unwrap_or_default())
        }
        _ => error(404, "not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Adapter, Memory, Process, Sample};

    fn settings() -> Settings {
        Settings { interval: Duration::from_secs(2), retention: Duration::from_secs(600), uptime_seconds: 7 }
    }

    fn at(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000 + seconds).unwrap()
    }

    fn adapter(used: u64) -> Adapter {
        Adapter {
            index: 0,
            name: "GPU".into(),
            utilization_percent: Some(12),
            memory: Memory { used_mib: used, total_mib: 16000 },
            temperature_c: None,
            power_watts: Some(30.5),
            processes: vec![Process { pid: 9, name: "python.exe".into(), memory_mib: 2000 }],
        }
    }

    fn store_with(samples: &[(i64, Vec<Adapter>)]) -> Store {
        let mut store = Store::new(Duration::from_secs(2), Duration::from_secs(600));
        for (seconds, adapters) in samples {
            store.push(Sample { at: at(*seconds), adapters: adapters.clone() });
        }
        store
    }

    fn body(response: &(u16, String)) -> serde_json::Value {
        serde_json::from_str(&response.1).unwrap()
    }

    #[test]
    fn health_answers_without_a_gpu() {
        let response = route("GET", "/health", &store_with(&[]), &settings(), at(0));
        assert_eq!(response.0, 200);
        assert_eq!(body(&response)["status"], "ok");
        assert_eq!(body(&response)["uptime_seconds"], 7);
    }

    #[test]
    fn the_latest_sample_follows_the_contract() {
        let store = store_with(&[(0, vec![adapter(100)]), (2, vec![adapter(5560)])]);
        let response = route("GET", "/v1/gpu", &store, &settings(), at(3));
        let value = body(&response);
        assert_eq!(response.0, 200);
        assert_eq!(value["schema"], 1);
        assert_eq!(value["available"], true);
        assert_eq!(value["interval_ms"], 2000);
        assert_eq!(value["adapters"][0]["memory"]["used_mib"], 5560);
        assert_eq!(value["adapters"][0]["temperature_c"], serde_json::Value::Null);
        assert_eq!(value["adapters"][0]["processes"][0]["name"], "python.exe");
        assert!(value["sampled_at"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn no_gpu_is_200_and_not_available() {
        let response = route("GET", "/v1/gpu", &store_with(&[(0, vec![])]), &settings(), at(1));
        assert_eq!(response.0, 200);
        assert_eq!(body(&response)["available"], false);
        assert_eq!(body(&response)["adapters"], json!([]));
        // Before the first sample the answer is the same.
        let first = route("GET", "/v1/gpu", &store_with(&[]), &settings(), at(1));
        assert_eq!((first.0, body(&first)["available"].clone()), (200, json!(false)));
    }

    #[test]
    fn the_history_is_oldest_first_and_compact() {
        let store = store_with(&[(0, vec![adapter(1)]), (2, vec![adapter(2)]), (4, vec![adapter(3)])]);
        let response = route("GET", "/v1/gpu/history?seconds=3", &store, &settings(), at(5));
        let value = body(&response);
        assert_eq!(response.0, 200);
        let used: Vec<u64> = value["samples"].as_array().unwrap().iter().map(|s| s["adapters"][0]["memory_used_mib"].as_u64().unwrap()).collect();
        // now is 5 s, so the last 3 seconds start at 2 s: the samples of 2 s and 4 s, oldest first.
        assert_eq!(used, vec![2, 3]);
        assert!(value["samples"][0]["adapters"][0].get("name").is_none());
        assert_eq!(value["retention_seconds"], 600);
    }

    #[test]
    fn the_history_defaults_to_the_whole_retention_and_caps_seconds() {
        let store = store_with(&[(0, vec![adapter(1)]), (2, vec![adapter(2)])]);
        let all = body(&route("GET", "/v1/gpu/history", &store, &settings(), at(3)));
        assert_eq!(all["samples"].as_array().unwrap().len(), 2);
        let capped = body(&route("GET", "/v1/gpu/history?seconds=99999", &store, &settings(), at(3)));
        assert_eq!(capped["samples"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_bad_seconds_is_400() {
        for url in ["/v1/gpu/history?seconds=0", "/v1/gpu/history?seconds=-5", "/v1/gpu/history?seconds=abc", "/v1/gpu/history?seconds="] {
            let response = route("GET", url, &store_with(&[]), &settings(), at(0));
            assert_eq!(response.0, 400, "{url}");
            assert!(body(&response)["error"].is_string());
        }
    }

    #[test]
    fn unknown_paths_and_methods_are_404() {
        assert_eq!(route("GET", "/nope", &store_with(&[]), &settings(), at(0)).0, 404);
        assert_eq!(route("POST", "/v1/gpu", &store_with(&[]), &settings(), at(0)).0, 404);
    }
}
