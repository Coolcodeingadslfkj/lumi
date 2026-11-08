//! Model router: sends each job to the best AI for it, with automatic fallback.
//!
//! Paid mode: everything goes to Claude. Free mode: each job has an ordered list of free
//! providers; if one is missing a key, rate-limited, overloaded, or returns unusable output,
//! the job moves to the next. All providers are called over plain HTTPS (no SDKs exist for Rust).

use crate::db::Db;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

pub type Provider = &'static str;

pub fn label(p: &str) -> &'static str {
    match p {
        "claude" => "Claude",
        "gemini" => "Gemini",
        "groq" => "Groq",
        "mistral" => "Mistral",
        "openrouter" => "OpenRouter",
        _ => "AI",
    }
}

/// Which free AI does which job, best first. Chosen from hands-on tests of each free tier:
/// Gemini reads PDF pages natively (math + figures) and sees photos; Groq answers in ~2 s but
/// allows only ~8k tokens/minute; Mistral Large was the most careful grader; OpenRouter's free
/// models are slower and capped per day, so they're the last resort.
pub const ROUTES: &[(&str, &str, &[Provider])] = &[
    ("read", "Reading your materials", &["gemini", "mistral", "openrouter"]),
    ("teach", "Teaching & photo help", &["gemini", "mistral", "openrouter", "groq"]),
    ("quiz", "Practice questions", &["groq", "gemini", "mistral", "openrouter"]),
    ("exam", "Mock exams", &["gemini", "mistral", "groq", "openrouter"]),
    ("variants", "Mistake re-quizzes", &["groq", "mistral", "gemini", "openrouter"]),
    ("explain", "\u{201c}Explain this\u{201d}", &["groq", "mistral", "gemini", "openrouter"]),
    ("grade", "Grading written answers", &["mistral", "groq", "gemini", "openrouter"]),
];

pub const CLAUDE_MODELS: &[(&str, &str)] = &[
    ("claude-opus-5-5", "Claude Opus 5.5 (best teaching quality)"),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5 (faster, cheaper)"),
    ("claude-haiku-5-5", "Claude Haiku 5.5 (fastest, cheapest)"),
];
pub const GEMINI_MODELS: &[(&str, &str)] = &[
    ("gemini-3.5-flash", "Gemini 3.5 Flash (recommended)"),
    ("gemini-3.8-flash", "Gemini 3.8 Flash (newest, sometimes busy)"),
    ("gemini-3.5-flash-lite", "Gemini 3.5 Flash-Lite (fastest)"),
];
const GEMINI_FALLBACKS: &[&str] = &["gemini-3.5-flash", "gemini-3.5-flash-lite"];

struct Compat {
    base: &'static str,
    models: &'static [&'static str],
    budget: usize,       // characters of document text per message
    tpm: Option<usize>,  // per-minute token limit that counts prompt + max_tokens (Groq)
    vision: &'static [&'static str],
}

fn compat(p: &str) -> Option<&'static Compat> {
    static GROQ: Compat = Compat { base: "https://api.groq.com/openai/v1", models: &["openai/gpt-oss-120b", "qwen/qwen3.8-27b"], budget: 6_000, tpm: Some(8_000), vision: &[] };
    static MISTRAL: Compat = Compat { base: "https://api.mistral.ai/v1", models: &["mistral-large-4"], budget: 24_000, tpm: None, vision: &["mistral-large-4"] };
    static OPENROUTER: Compat = Compat {
        base: "https://openrouter.ai/api/v1",
        models: &["nvidia/nemotron-3-ultra-550b-a55b:free", "nvidia/nemotron-3.5-super-vl:free", "google/gemma-4-31b-it:free"],
        budget: 60_000,
        tpm: None,
        vision: &["nvidia/nemotron-3.5-super-vl:free", "google/gemma-4-31b-it:free"],
    };
    match p {
        "groq" => Some(&GROQ),
        "mistral" => Some(&MISTRAL),
        "openrouter" => Some(&OPENROUTER),
        _ => None,
    }
}

// ---------- messages ----------

pub enum Pdf {
    File(PathBuf),
    Bytes(Vec<u8>),
}

pub enum Part {
    /// `aux` = optional extra (like a textbook's table of contents) dropped for AIs with tight limits.
    Text { text: String, aux: bool },
    /// A document: PDF-capable AIs get the real pages, text-only AIs get `text`.
    Doc { title: String, text: String, pdf: Option<Pdf> },
    Image { data: Vec<u8>, mime: String },
}

impl Part {
    pub fn text(s: impl Into<String>) -> Part {
        Part::Text { text: s.into(), aux: false }
    }
}

pub struct Msg {
    pub user: bool,
    pub parts: Vec<Part>,
    /// Stored Claude content blocks (with thinking signatures) to replay to Claude unchanged.
    pub raw: Option<Value>,
}

impl Msg {
    pub fn user(parts: Vec<Part>) -> Msg {
        Msg { user: true, parts, raw: None }
    }
}

pub enum Ev {
    Provider(Provider),
    Thinking(String),
    Text(String),
    /// The AI failed mid-reply; the answer restarts on the next one.
    Reset,
}

pub struct StreamOut {
    pub provider: Provider,
    pub text: String,
    /// Claude only: (content blocks, stop_reason) to store for replay.
    pub claude: Option<(Value, String)>,
}

// ---------- errors ----------

#[derive(Debug)]
pub enum LlmError {
    NoKey,
    Api { provider: Provider, status: u16, message: String, retry_after: Option<u64> },
    Net { provider: Provider, message: String },
    BadOutput(String),
    Declined(String),
    AllFailed(Vec<(Provider, LlmError)>),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LlmError::NoKey => write!(f, "no key"),
            LlmError::Api { provider, status, message, .. } => write!(f, "{} {status}: {}", label(provider), message.chars().take(200).collect::<String>()),
            LlmError::Net { provider, message } => write!(f, "{}: {message}", label(provider)),
            LlmError::BadOutput(m) => write!(f, "bad output: {m}"),
            LlmError::Declined(m) => write!(f, "{m}"),
            LlmError::AllFailed(v) => write!(f, "{}", v.iter().map(|(p, e)| format!("{}: {e}", label(p))).collect::<Vec<_>>().join("; ")),
        }
    }
}

impl LlmError {
    fn status(&self) -> Option<u16> {
        match self {
            LlmError::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn friendly(&self, free_mode: bool) -> String {
        match self {
            LlmError::NoKey => {
                if free_mode {
                    "No AI key yet. Add a free key (Gemini, Groq, Mistral or OpenRouter) in Settings to wake Lumi up.".into()
                } else {
                    "No Claude API key yet. Add one in Settings to wake Lumi up.".into()
                }
            }
            LlmError::AllFailed(errs) => {
                if errs.iter().all(|(_, e)| e.status() == Some(429)) {
                    "All the free AIs are at their limit right now. Wait a minute and try again. If it keeps happening, today's free quota is used up and resets tomorrow.".into()
                } else {
                    let who: Vec<&str> = errs.iter().map(|(p, _)| label(p)).collect();
                    format!("None of the AIs could finish this ({} all had problems). Please try again in a moment.", who.join(", "))
                }
            }
            LlmError::Declined(m) => m.clone(),
            LlmError::BadOutput(_) => "The AI's answer came back garbled. Please try again.".into(),
            LlmError::Net { provider, .. } => format!("Can't reach {}. Check your internet connection.", label(provider)),
            LlmError::Api { provider, status, message, .. } => match status {
                429 => format!("{} is rate-limiting us for a moment. Wait a few seconds and try again.", label(provider)),
                401 | 403 => format!("{} rejected the API key. Double-check it in Settings.", label(provider)),
                500..=599 => format!("{} is overloaded right now. Try again in a moment.", label(provider)),
                _ => format!("{} couldn't do that ({status}): {}", label(provider), message.chars().take(200).collect::<String>()),
            },
        }
    }
}

type R<T> = Result<T, LlmError>;

// ---------- routing ----------

static COOLDOWN: LazyLock<Mutex<HashMap<Provider, Instant>>> = LazyLock::new(Default::default);

fn note_failure(p: Provider, e: &LlmError) {
    if let LlmError::Api { status: 429, message, retry_after, .. } = e {
        let wait = if message.contains("PerDay") || message.to_lowercase().contains("per day") {
            3600
        } else {
            retry_after.map(|r| r + 2).unwrap_or(45)
        };
        COOLDOWN.lock().unwrap().insert(p, Instant::now() + Duration::from_secs(wait.min(3600)));
    }
}

pub fn route(db: &Db, job: &str) -> Vec<Provider> {
    if db.mode() == "claude" {
        return vec!["claude"];
    }
    ROUTES.iter().find(|r| r.0 == job).map(|r| r.2.to_vec()).unwrap_or_else(|| vec!["gemini"])
}

fn available(db: &Db, job: &str) -> R<Vec<Provider>> {
    let names: Vec<Provider> = route(db, job).into_iter().filter(|p| db.key(p).is_some()).collect();
    if names.is_empty() {
        return Err(LlmError::NoKey);
    }
    let now = Instant::now();
    let cd = COOLDOWN.lock().unwrap();
    let ready: Vec<Provider> = names.iter().copied().filter(|p| cd.get(p).map(|t| *t <= now).unwrap_or(true)).collect();
    let cooling: Vec<Provider> = names.iter().copied().filter(|p| !ready.contains(p)).collect();
    Ok(ready.into_iter().chain(cooling).collect()) // cooling providers last, not never
}

fn combine(mut errors: Vec<(Provider, LlmError)>) -> LlmError {
    if errors.len() == 1 {
        errors.pop().unwrap().1
    } else {
        LlmError::AllFailed(errors)
    }
}

/// Structured output from the first provider that delivers.
pub fn json_call(db: &Db, job: &str, system: &str, msgs: &[Msg], schema: &Value, max_tokens: u32, effort: &str) -> R<(Value, Provider)> {
    let mut errors = vec![];
    for p in available(db, job)? {
        let key = db.key(p).unwrap_or_default();
        let r = match p {
            "claude" => claude_json(db, &key, system, msgs, schema, effort),
            "gemini" => gemini_json(db, &key, system, msgs, schema, max_tokens),
            _ => compat_json(p, &key, system, msgs, schema, max_tokens),
        };
        match r.and_then(|v| {
            let missing: Vec<&str> = schema["required"].as_array().into_iter().flatten().filter_map(|k| k.as_str()).filter(|k| v.get(*k).is_none()).collect();
            if missing.is_empty() { Ok(v) } else { Err(LlmError::BadOutput(format!("missing {missing:?}"))) }
        }) {
            Ok(v) => return Ok((v, p)),
            Err(e) => {
                eprintln!("[lumi] {} couldn't do '{job}' ({e}); trying the next AI", label(p));
                note_failure(p, &e);
                if p == "claude" {
                    return Err(e);
                }
                errors.push((p, e));
            }
        }
    }
    Err(combine(errors))
}

/// Streamed reply; falls back to the next provider on failure (emitting Ev::Reset if text was already shown).
pub fn stream(db: &Db, job: &str, system: &str, msgs: &[Msg], show_thinking: bool, on: &mut dyn FnMut(Ev)) -> R<StreamOut> {
    let mut errors = vec![];
    for p in available(db, job)? {
        let key = db.key(p).unwrap_or_default();
        on(Ev::Provider(p));
        let mut started = false;
        let mut text = String::new();
        let mut cb = |ev: Ev| {
            if let Ev::Text(t) = &ev {
                started = true;
                text.push_str(t);
            }
            if let Ev::Reset = &ev {
                text.clear();
            }
            on(ev)
        };
        let r = match p {
            "claude" => claude_stream(&key, db, system, msgs, show_thinking, &mut cb),
            "gemini" => gemini_stream(db, &key, system, msgs, show_thinking, &mut cb).map(|_| None),
            _ => compat_stream(p, &key, system, msgs, &mut cb).map(|_| None),
        };
        match r {
            Ok(claude) if started => return Ok(StreamOut { provider: p, text, claude }),
            Ok(_) => errors.push((p, LlmError::BadOutput("empty reply".into()))),
            Err(e) => {
                eprintln!("[lumi] {} couldn't do '{job}' ({e}); trying the next AI", label(p));
                note_failure(p, &e);
                if p == "claude" {
                    return Err(e);
                }
                if started {
                    on(Ev::Reset);
                }
                errors.push((p, e));
            }
        }
    }
    Err(combine(errors))
}

// ---------- HTTP helpers ----------

fn agent() -> &'static ureq::Agent {
    static A: LazyLock<ureq::Agent> = LazyLock::new(|| {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_global(Some(Duration::from_secs(600)))
            .build()
            .into()
    });
    &A
}

type Resp = ureq::http::Response<ureq::Body>;

fn post(provider: Provider, url: &str, headers: &[(&str, &str)], body: &Value) -> R<Resp> {
    let mut req = agent().post(url).header("content-type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send(serde_json::to_vec(body).unwrap()).map_err(|e| LlmError::Net { provider, message: e.to_string() })?;
    check(provider, resp)
}

fn check(provider: Provider, mut resp: Resp) -> R<Resp> {
    let status = resp.status().as_u16();
    if status < 400 {
        return Ok(resp);
    }
    let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<f64>().ok()).map(|f| f.ceil() as u64);
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    let message = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(String::from).or_else(|| v["message"].as_str().map(String::from)))
        .unwrap_or(body.clone());
    // Gemini puts the retry delay inside the error details
    let retry_after = retry_after.or_else(|| {
        let i = body.find("retryDelay")?;
        body[i..].chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
    });
    let message = if body.contains("PerDay") { format!("{message} (PerDay)") } else { message };
    Err(LlmError::Api { provider, status, message, retry_after })
}

/// Server-sent events: calls `f(event_name, data)` for each event.
fn sse(provider: Provider, resp: Resp, mut f: impl FnMut(&str, &str) -> R<bool>) -> R<()> {
    let mut reader = BufReader::new(resp.into_body().into_reader());
    let (mut event, mut data) = (String::new(), String::new());
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| LlmError::Net { provider, message: e.to_string() })?;
        let l = line.trim_end_matches(['\r', '\n']);
        if n == 0 || l.is_empty() {
            if !data.is_empty() {
                if !f(&event, &data)? {
                    return Ok(());
                }
            }
            event.clear();
            data.clear();
            if n == 0 {
                return Ok(());
            }
            continue;
        }
        if let Some(v) = l.strip_prefix("event:") {
            event = v.trim().to_string();
        } else if let Some(v) = l.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(v.strip_prefix(' ').unwrap_or(v));
        }
    }
}

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn pdf_bytes(p: &Pdf) -> Option<Vec<u8>> {
    match p {
        Pdf::File(path) => std::fs::read(path).ok(),
        Pdf::Bytes(b) => Some(b.clone()),
    }
}

/// Flatten parts to text for text-only AIs; documents share the character budget.
fn text_of(parts: &[Part], budget: usize) -> String {
    let docs = parts.iter().filter(|p| matches!(p, Part::Doc { .. })).count().max(1);
    let share = budget / docs;
    let mut out = vec![];
    for p in parts {
        match p {
            Part::Text { aux: true, .. } if budget < 20_000 => {}
            Part::Text { text, .. } => out.push(text.clone()),
            Part::Doc { title, text, .. } => {
                let body: String = if text.chars().count() > share {
                    text.chars().take(share).collect::<String>() + "\n[... truncated to fit this AI's limits ...]"
                } else {
                    text.clone()
                };
                out.push(format!("<document title=\"{title}\">\n{body}\n</document>"));
            }
            Part::Image { .. } => {}
        }
    }
    out.join("\n\n")
}

pub fn parse_json(text: &str) -> R<Value> {
    let t = text.trim();
    let t = t.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    if let Ok(v) = serde_json::from_str(t) {
        return Ok(v);
    }
    if let (Some(i), Some(j)) = (t.find('{'), t.rfind('}')) {
        if j > i {
            if let Ok(v) = serde_json::from_str(&t[i..=j]) {
                return Ok(v);
            }
        }
    }
    Err(LlmError::BadOutput("invalid JSON".into()))
}

// ---------- Claude (paid) ----------

const ANTHROPIC_VERSION: &str = "2026-10-21";

pub fn claude_model(db: &Db) -> String {
    let m: String = db.get_or("model", "claude-opus-5-5".to_string());
    if CLAUDE_MODELS.iter().any(|x| x.0 == m) { m } else { "claude-opus-5-5".into() }
}

fn claude_messages(msgs: &[Msg]) -> Vec<Value> {
    let mut out = vec![];
    for (i, m) in msgs.iter().enumerate() {
        if !m.user {
            let content = m.raw.clone().filter(|r| r.as_array().map(|a| !a.is_empty()).unwrap_or(false)).unwrap_or_else(|| json!([{"type": "text", "text": text_of(&m.parts, usize::MAX)}]));
            out.push(json!({"role": "assistant", "content": content}));
            continue;
        }
        let mut blocks: Vec<Value> = vec![];
        for p in &m.parts {
            match p {
                Part::Text { text, .. } => blocks.push(json!({"type": "text", "text": text})),
                Part::Doc { title, text, pdf } => match pdf.as_ref().and_then(pdf_bytes) {
                    Some(b) => blocks.push(json!({"type": "document", "title": title, "source": {"type": "base64", "media_type": "application/pdf", "data": b64(&b)}})),
                    None => blocks.push(json!({"type": "document", "title": title, "source": {"type": "text", "media_type": "text/plain", "data": if text.is_empty() { "(empty)" } else { text }}})),
                },
                Part::Image { data, mime } => blocks.push(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": b64(data)}})),
            }
        }
        if i == 0 {
            if let Some(last) = blocks.last_mut() {
                last["cache_control"] = json!({"type": "ephemeral"}); // the first message carries the materials
            }
        }
        out.push(json!({"role": "user", "content": blocks}));
    }
    out
}

fn claude_request(db: &Db, key: &str, system: &str, msgs: &[Msg], effort: &str, show_thinking: bool, chat: bool, max_tokens: u32, format: Option<&Value>) -> R<Resp> {
    let model = claude_model(db);
    let mut thinking = json!({"type": "adaptive"});
    if show_thinking {
        thinking["display"] = json!("summarized");
    }
    let mut betas = vec![];
    let mut body = json!({
        "model": model, "max_tokens": max_tokens, "system": system, "stream": true,
        "messages": claude_messages(msgs), "output_config": {"effort": effort},
    });
    if model != "claude-haiku-5-5" {
        betas.push("server-side-fallback-2026-07-01"); // retry on another Claude model if one declines
        body["fallbacks"] = json!("default");
    }
    if chat {
        // chats replay thinking blocks; if earlier turns ever differ, drop stale blocks instead of failing
        thinking["block_binding"] = json!({"prefix_mismatch_behavior": "drop_block"});
        betas.push("thinking-binding-controls-2026-08-01");
        body["cache_control"] = json!({"type": "ephemeral"});
    }
    body["thinking"] = thinking;
    if let Some(f) = format {
        body["output_config"]["format"] = json!({"type": "json_schema", "schema": f});
    }
    let beta = betas.join(",");
    let mut headers = vec![("x-api-key", key), ("anthropic-version", ANTHROPIC_VERSION)];
    if !beta.is_empty() {
        headers.push(("anthropic-beta", &beta));
    }
    post("claude", "https://api.anthropic.com/v1/messages", &headers, &body)
}

/// Reads a Claude stream, rebuilding the content blocks. Returns (blocks, stop_reason).
fn claude_read(resp: Resp, on: &mut dyn FnMut(Ev)) -> R<(Value, String)> {
    let mut blocks: Vec<Value> = vec![];
    let mut stop = String::new();
    sse("claude", resp, |event, data| {
        let v: Value = serde_json::from_str(data).unwrap_or(Value::Null);
        match v["type"].as_str().unwrap_or(event) {
            "content_block_start" => {
                let i = v["index"].as_u64().unwrap_or(0) as usize;
                if blocks.len() <= i {
                    blocks.resize(i + 1, Value::Null);
                }
                blocks[i] = v["content_block"].clone();
            }
            "content_block_delta" => {
                let i = v["index"].as_u64().unwrap_or(0) as usize;
                let d = &v["delta"];
                if let Some(b) = blocks.get_mut(i) {
                    match d["type"].as_str() {
                        Some("text_delta") => {
                            let t = d["text"].as_str().unwrap_or("");
                            b["text"] = json!(format!("{}{t}", b["text"].as_str().unwrap_or("")));
                            on(Ev::Text(t.to_string()));
                        }
                        Some("thinking_delta") => {
                            let t = d["thinking"].as_str().unwrap_or("");
                            b["thinking"] = json!(format!("{}{t}", b["thinking"].as_str().unwrap_or("")));
                            if !t.is_empty() {
                                on(Ev::Thinking(t.to_string()));
                            }
                        }
                        Some("signature_delta") => {
                            let t = d["signature"].as_str().unwrap_or("");
                            b["signature"] = json!(format!("{}{t}", b["signature"].as_str().unwrap_or("")));
                        }
                        _ => {}
                    }
                }
            }
            "message_delta" => {
                if let Some(s) = v["delta"]["stop_reason"].as_str() {
                    stop = s.to_string();
                }
            }
            "error" => {
                return Err(LlmError::Api { provider: "claude", status: 529, message: v["error"]["message"].as_str().unwrap_or("stream error").into(), retry_after: None });
            }
            _ => {}
        }
        Ok(true)
    })?;
    if stop == "refusal" {
        return Err(LlmError::Declined("Lumi can't help with that one. Try rephrasing.".into()));
    }
    Ok((Value::Array(blocks.into_iter().filter(|b| !b.is_null()).collect()), stop))
}

fn claude_json(db: &Db, key: &str, system: &str, msgs: &[Msg], schema: &Value, effort: &str) -> R<Value> {
    let resp = claude_request(db, key, system, msgs, effort, false, false, 32000, Some(schema))?;
    let (blocks, stop) = claude_read(resp, &mut |_| {})?;
    if stop == "max_tokens" {
        return Err(LlmError::BadOutput("cut off".into()));
    }
    let text: String = blocks.as_array().unwrap().iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect();
    parse_json(&text)
}

fn claude_stream(key: &str, db: &Db, system: &str, msgs: &[Msg], show_thinking: bool, on: &mut dyn FnMut(Ev)) -> R<Option<(Value, String)>> {
    let (effort, max) = if show_thinking { ("medium", 32000) } else { ("low", 8000) };
    let resp = claude_request(db, key, system, msgs, effort, show_thinking, show_thinking, max, None)?;
    claude_read(resp, on).map(Some)
}

// ---------- Gemini ----------

pub fn gemini_model(db: &Db) -> String {
    let m: String = db.get_or("gemini_model", "gemini-3.5-flash".to_string());
    if GEMINI_MODELS.iter().any(|x| x.0 == m) { m } else { "gemini-3.5-flash".into() }
}

fn gemini_models(db: &Db) -> Vec<String> {
    let first = gemini_model(db);
    std::iter::once(first.clone()).chain(GEMINI_FALLBACKS.iter().filter(|m| **m != first).map(|m| m.to_string())).collect()
}

const GEMINI_INLINE_LIMIT: usize = 16 * 1024 * 1024; // keep requests under Gemini's ~20 MB inline cap

fn gemini_contents(msgs: &[Msg]) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    let mut inline = 0usize;
    for m in msgs {
        let mut parts: Vec<Value> = vec![];
        if m.user {
            for p in &m.parts {
                match p {
                    Part::Text { text, .. } => parts.push(json!({"text": text})),
                    Part::Doc { title, text, pdf } => {
                        let bytes = pdf.as_ref().and_then(pdf_bytes).filter(|b| inline + b.len() < GEMINI_INLINE_LIMIT);
                        match bytes {
                            Some(b) => {
                                inline += b.len();
                                parts.push(json!({"text": format!("[Document: {title}]")}));
                                parts.push(json!({"inlineData": {"mimeType": "application/pdf", "data": b64(&b)}}));
                            }
                            None => parts.push(json!({"text": format!("<document title=\"{title}\">\n{text}\n</document>")})),
                        }
                    }
                    Part::Image { data, mime } => parts.push(json!({"inlineData": {"mimeType": mime, "data": b64(data)}})),
                }
            }
        } else {
            let t = text_of(&m.parts, usize::MAX);
            if !t.is_empty() {
                parts.push(json!({"text": t}));
            }
        }
        if parts.is_empty() {
            continue;
        }
        let role = if m.user { "user" } else { "model" };
        if let Some(last) = out.last_mut() {
            if last["role"] == role {
                last["parts"].as_array_mut().unwrap().extend(parts);
                continue;
            }
        }
        out.push(json!({"role": role, "parts": parts}));
    }
    out
}

fn gemini_json(db: &Db, key: &str, system: &str, msgs: &[Msg], schema: &Value, max_tokens: u32) -> R<Value> {
    let body = json!({
        "systemInstruction": {"parts": [{"text": system}]},
        "contents": gemini_contents(msgs),
        "generationConfig": {"responseMimeType": "application/json", "responseJsonSchema": schema, "maxOutputTokens": max_tokens.max(16000)},
    });
    let mut last = None;
    for m in gemini_models(db) {
        let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{m}:generateContent");
        match post("gemini", &url, &[("x-goog-api-key", key)], &body) {
            Ok(mut resp) => {
                let v: Value = resp.body_mut().read_json().map_err(|e| LlmError::BadOutput(e.to_string()))?;
                if v["promptFeedback"]["blockReason"].is_string() {
                    return Err(LlmError::Declined("Gemini declined this request. Try rephrasing or using different materials.".into()));
                }
                let cand = &v["candidates"][0];
                match cand["finishReason"].as_str() {
                    Some("SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST") => return Err(LlmError::Declined("Gemini declined this request.".into())),
                    Some("MAX_TOKENS") => return Err(LlmError::BadOutput("cut off".into())),
                    _ => {}
                }
                let text: String = cand["content"]["parts"].as_array().into_iter().flatten().filter(|p| p["thought"] != true).filter_map(|p| p["text"].as_str()).collect();
                return parse_json(&text);
            }
            Err(e @ LlmError::Api { status: 404 | 500..=599, .. }) => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or(LlmError::BadOutput("no model".into())))
}

fn gemini_stream(db: &Db, key: &str, system: &str, msgs: &[Msg], show_thinking: bool, on: &mut dyn FnMut(Ev)) -> R<()> {
    let body = json!({
        "systemInstruction": {"parts": [{"text": system}]},
        "contents": gemini_contents(msgs),
        "generationConfig": {"maxOutputTokens": 32000, "thinkingConfig": {"includeThoughts": show_thinking}},
    });
    let models = gemini_models(db);
    for (i, m) in models.iter().enumerate() {
        let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{m}:streamGenerateContent?alt=sse");
        let mut started = false;
        let r = post("gemini", &url, &[("x-goog-api-key", key)], &body).and_then(|resp| {
            sse("gemini", resp, |_, data| {
                let v: Value = serde_json::from_str(data).unwrap_or(Value::Null);
                if let Some(err) = v.get("error") {
                    return Err(LlmError::Api { provider: "gemini", status: err["code"].as_u64().unwrap_or(500) as u16, message: err["message"].as_str().unwrap_or("").into(), retry_after: None });
                }
                for p in v["candidates"][0]["content"]["parts"].as_array().into_iter().flatten() {
                    if let Some(t) = p["text"].as_str() {
                        if p["thought"] == true {
                            on(Ev::Thinking(t.into()));
                        } else {
                            started = true;
                            on(Ev::Text(t.into()));
                        }
                    }
                }
                Ok(true)
            })
        });
        match r {
            Ok(()) => return Ok(()),
            Err(LlmError::Api { status: 404 | 500..=599, .. }) | Err(LlmError::Net { .. }) if i + 1 < models.len() => {
                if started {
                    on(Ev::Reset);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ---------- Groq / Mistral / OpenRouter (OpenAI-compatible) ----------

fn compat_messages(c: &Compat, model: &str, system: &str, msgs: &[Msg]) -> Vec<Value> {
    let mut out = vec![json!({"role": "system", "content": system})];
    // text-only AIs re-read the whole conversation as text each turn: keep documents only in
    // the two most recent messages that have them, so long lessons stay within their limits
    let with_docs: Vec<usize> = msgs.iter().enumerate().filter(|(_, m)| m.parts.iter().any(|p| matches!(p, Part::Doc { .. }))).map(|(i, _)| i).collect();
    let keep: Vec<usize> = with_docs.iter().rev().take(2).copied().collect();
    for (i, m) in msgs.iter().enumerate() {
        let parts: Vec<Part> = m
            .parts
            .iter()
            .map(|p| match p {
                Part::Doc { title, .. } if with_docs.contains(&i) && !keep.contains(&i) => Part::text(format!("[Earlier excerpt: {title}]")),
                Part::Text { text, aux } => Part::Text { text: text.clone(), aux: *aux },
                Part::Doc { title, text, .. } => Part::Doc { title: title.clone(), text: text.clone(), pdf: None },
                Part::Image { data, mime } => Part::Image { data: data.clone(), mime: mime.clone() },
            })
            .collect();
        let mut text = text_of(&parts, c.budget);
        let images: Vec<&Part> = parts.iter().filter(|p| matches!(p, Part::Image { .. })).collect();
        if m.user && !images.is_empty() {
            if c.vision.contains(&model) {
                let mut content: Vec<Value> = images
                    .iter()
                    .filter_map(|p| if let Part::Image { data, mime } = p { Some(json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{}", b64(data))}})) } else { None })
                    .collect();
                content.push(json!({"type": "text", "text": text}));
                out.push(json!({"role": "user", "content": content}));
                continue;
            }
            text = format!("[The learner attached a photo, but this AI can't see images. Ask them to describe it.]\n{text}");
        }
        out.push(json!({"role": if m.user { "user" } else { "assistant" }, "content": text}));
    }
    out
}

fn content_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter(|x| x["type"] == "text").filter_map(|x| x["text"].as_str()).collect(),
        _ => String::new(),
    }
}

fn compat_json(p: Provider, key: &str, system: &str, msgs: &[Msg], schema: &Value, max_tokens: u32) -> R<Value> {
    let c = compat(p).unwrap();
    let auth = format!("Bearer {key}");
    let mut last = None;
    for (mi, model) in c.models.iter().enumerate() {
        let mut messages = compat_messages(c, model, system, msgs);
        let mut mt = max_tokens as usize;
        if let Some(tpm) = c.tpm {
            let est = serde_json::to_string(&messages).unwrap().len() / 3;
            mt = mt.min(tpm.saturating_sub(est + 300));
            if mt < 1500 {
                return Err(LlmError::BadOutput("too large for this AI's limits".into()));
            }
        }
        let mut body = json!({"model": model, "messages": messages, "max_tokens": mt,
            "response_format": {"type": "json_schema", "json_schema": {"name": "result", "schema": schema, "strict": true}}});
        if model.starts_with("openai/gpt-oss") {
            body["reasoning_effort"] = json!("low");
        }
        let url = format!("{}/chat/completions", c.base);
        let mut r = post(p, &url, &[("authorization", &auth)], &body);
        if let Err(LlmError::Api { status: 400, .. }) = r {
            // model without structured outputs: ask for plain JSON and validate ourselves
            messages[0] = json!({"role": "system", "content": format!("{system}\n\nReply with only a JSON object matching this schema:\n{schema}")});
            body["messages"] = json!(messages);
            body["response_format"] = json!({"type": "json_object"});
            r = post(p, &url, &[("authorization", &auth)], &body);
        }
        match r {
            Ok(mut resp) => {
                let v: Value = resp.body_mut().read_json().map_err(|e| LlmError::BadOutput(e.to_string()))?;
                if v["choices"][0]["finish_reason"] == "length" {
                    last = Some(LlmError::BadOutput("cut off".into()));
                    continue;
                }
                return parse_json(&content_text(&v["choices"][0]["message"]["content"]));
            }
            Err(e @ LlmError::Api { status: 404 | 500..=599, .. }) => last = Some(e),
            Err(e @ LlmError::Api { status: 429, .. }) if mi + 1 < c.models.len() => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or(LlmError::BadOutput("no model".into())))
}

fn compat_stream(p: Provider, key: &str, system: &str, msgs: &[Msg], on: &mut dyn FnMut(Ev)) -> R<()> {
    let c = compat(p).unwrap();
    let auth = format!("Bearer {key}");
    for (i, model) in c.models.iter().enumerate() {
        let mut body = json!({"model": model, "messages": compat_messages(c, model, system, msgs),
            "max_tokens": if c.tpm.is_some() { 4000 } else { 8000 }, "stream": true});
        if model.starts_with("openai/gpt-oss") {
            body["reasoning_effort"] = json!("low");
        }
        let mut started = false;
        let r = post(p, &format!("{}/chat/completions", c.base), &[("authorization", &auth)], &body).and_then(|resp| {
            sse(p, resp, |_, data| {
                if data.trim() == "[DONE]" {
                    return Ok(false);
                }
                let v: Value = serde_json::from_str(data).unwrap_or(Value::Null);
                let t = content_text(&v["choices"][0]["delta"]["content"]);
                if !t.is_empty() {
                    started = true;
                    on(Ev::Text(t));
                }
                Ok(true)
            })
        });
        match r {
            Ok(()) => return Ok(()),
            Err(LlmError::Api { status: 404 | 429 | 500..=599, .. }) if i + 1 < c.models.len() => {
                if started {
                    on(Ev::Reset);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ---------- key check (Settings) ----------

/// (ok, message) per free provider; ok=None when there's no key.
pub fn test_keys(db: &Db) -> Vec<(Provider, Option<bool>, String)> {
    crate::db::FREE_PROVIDERS
        .iter()
        .map(|p| {
            let Some(key) = db.key(p) else { return (*p, None, "No key".to_string()) };
            let req = if *p == "gemini" {
                agent().get("https://generativelanguage.googleapis.com/v1beta/models?pageSize=1").header("x-goog-api-key", &key)
            } else {
                agent().get(&format!("{}/models", compat(p).unwrap().base)).header("authorization", &format!("Bearer {key}"))
            };
            match req.call() {
                Ok(r) if r.status().as_u16() < 400 => (*p, Some(true), "Working".into()),
                Ok(r) => {
                    let s = r.status().as_u16();
                    (*p, Some(false), if matches!(s, 400 | 401 | 403) { "Key rejected".into() } else { format!("Problem ({s})") })
                }
                Err(e) => (*p, Some(false), format!("Can't connect ({e})")),
            }
        })
        .collect()
}

#[allow(dead_code)]
fn read_all(mut r: impl Read) -> String {
    let mut s = String::new();
    let _ = r.read_to_string(&mut s);
    s
}
