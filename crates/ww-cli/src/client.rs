//! HTTP client for the Workwright REST API.

use std::env;
use std::path::PathBuf;

use reqwest::blocking::Client as HttpClient;
use serde::de::DeserializeOwned;

use crate::cli::{CritArgs, TaskArgs, TasksArgs, TasteArgs};
use crate::output::{self, TaskJson, TasteJson, UserJson};

const DEFAULT_API: &str = "http://localhost:8077";

pub struct Client {
    http: HttpClient,
    base: String,
    token: Option<String>,
}

impl Client {
    pub fn from_env() -> Self {
        let base = env::var("WW_API").unwrap_or_else(|_| DEFAULT_API.to_string());
        let token = load_token();
        Self {
            http: HttpClient::new(),
            base: base.trim_end_matches('/').to_string(),
            token,
        }
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let url = format!("{}{}", self.base, path);
        let resp = self
            .http
            .get(&url)
            .send()
            .map_err(|e| format!("GET {url}: {e}"))?;
        parse_response(resp, "GET", &url)
    }

    fn post<T: DeserializeOwned>(&self, path: &str, body: &impl serde::Serialize) -> Result<T, String> {
        let url = format!("{}{}", self.base, path);
        let mut req = self.http.post(&url);
        if let Some(ref t) = self.token {
            req = req.bearer_auth(t);
        }
        let resp = req
            .json(body)
            .send()
            .map_err(|e| format!("POST {url}: {e}"))?;
        parse_response(resp, "POST", &url)
    }

    fn post_auth<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<T, String> {
        if self.token.is_none() {
            return Err(
                "no token — set WW_TOKEN or save token to ~/.workwright/token (register at /brief)"
                    .into(),
            );
        }
        self.post(path, body)
    }

    // --- Commands ---

    pub fn list_tasks(&self, args: &TasksArgs) -> Result<(), String> {
        let tasks: Vec<TaskJson> = self.get("/api/tasks")?;

        let filtered: Vec<&TaskJson> = tasks
            .iter()
            .filter(|t| {
                args.status
                    .as_deref()
                    .map_or(true, |s| t.status.eq_ignore_ascii_case(s))
            })
            .take(args.limit)
            .collect();

        if args.json {
            println!("{}", serde_json::to_string_pretty(&filtered).unwrap());
            return Ok(());
        }

        output::print_task_table(&filtered);
        Ok(())
    }

    pub fn create_task(&self, args: &TaskArgs) -> Result<(), String> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            intent: &'a str,
            why: &'a str,
            scope: Option<&'a str>,
        }

        let body = Req {
            intent: &args.intent,
            why: &args.why,
            scope: args.scope.as_deref(),
        };

        let task: TaskJson = self.post_auth("/api/tasks", &body)?;
        output::print_task_created(&task);
        Ok(())
    }

    pub fn submit_crit(&self, args: &CritArgs) -> Result<(), String> {
        #[derive(serde::Serialize)]
        struct Req {
            task_id: String,
            score: f64,
            reason: String,
        }

        let body = Req {
            task_id: args.task_id.clone(),
            score: args.score,
            reason: args.reason.clone(),
        };

        let resp: serde_json::Value = self.post_auth("/api/crit", &body)?;
        output::print_crit_result(&resp);
        Ok(())
    }

    pub fn show_taste(&self, args: &TasteArgs) -> Result<(), String> {
        if let Some(ref q) = args.query {
            let encoded = urlencoding(q);
            let resp: serde_json::Value = self.get(&format!("/api/taste/search?q={encoded}"))?;
            output::print_taste_search(&resp);
            return Ok(());
        }

        let path = if args.raw { "/api/taste?raw=true" } else { "/api/taste" };
        let data: TasteJson = self.get(path)?;
        println!("{}", data.text);
        eprintln!("\n({} signals)", data.signal_count);
        Ok(())
    }

    pub fn show_status(&self) -> Result<(), String> {
        let tasks: Vec<TaskJson> = self.get("/api/tasks")?;
        let users: Vec<UserJson> = self.get("/api/users")?;
        let taste: TasteJson = self.get("/api/taste?raw=true")?;
        output::print_status(&tasks, &users, taste.signal_count);
        Ok(())
    }

    pub fn show_me(&self) -> Result<(), String> {
        if self.token.is_none() {
            return Err("no token — set WW_TOKEN or save to ~/.workwright/token".into());
        }
        let me: serde_json::Value = self.get("/api/me")?;
        println!("{}", serde_json::to_string_pretty(&me).unwrap());
        Ok(())
    }

    pub fn list_users(&self) -> Result<(), String> {
        let users: Vec<UserJson> = self.get("/api/users")?;
        output::print_users(&users);
        Ok(())
    }
}

// --- Token ---

fn load_token() -> Option<String> {
    if let Ok(t) = env::var("WW_TOKEN") {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    let path = dirs_home().join(".workwright").join("token");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn dirs_home() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

// --- Helpers ---

fn parse_response<T: DeserializeOwned>(
    resp: reqwest::blocking::Response,
    method: &str,
    url: &str,
) -> Result<T, String> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("{method} {url} → {status}: {body}"));
    }
    resp.json::<T>()
        .map_err(|e| format!("{method} {url} → bad JSON: {e}"))
}

/// Minimal percent-encoding for query strings.
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}
