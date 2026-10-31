//! Thin bridge to the deepcode runtime.
//!
//! deepcode owns the agent loop, model, tools, approvals and session store. This
//! module only starts `deepcode serve --http`, forwards prompts, and projects the
//! runtime's SSE envelopes into the window-facing event stream. It deliberately
//! does not implement agent policy.

use serde::Serialize;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    net::TcpListener,
    process::{Child, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

use super::{
    persona::agent_workspace,
    resolver::{agent_command, child_path, missing_agent_message, resolve_agent_bin},
};
use crate::conversations::{
    initialize_current, load_history, verify_session_persisted, write_current,
};

const SERVER_START_TIMEOUT: Duration = Duration::from_secs(8);
const SERVER_POLL_INTERVAL: Duration = Duration::from_millis(80);
const SERVER_STOP_GRACE: Duration = Duration::from_secs(2);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnSnapshot {
    request_id: String,
    session_id: Option<String>,
    prompt: String,
    sequence: u64,
    assistant: String,
    tool_calls: usize,
    terminal: Option<String>,
    result: Option<Value>,
    error: Option<String>,
}

impl TurnSnapshot {
    fn record(&mut self, envelope: &Value) {
        self.sequence += 1;
        let item = envelope.get("item").unwrap_or(envelope);
        let payload = item.get("payload").unwrap_or(&Value::Null);
        match item.get("kind").and_then(Value::as_str).unwrap_or_default() {
            "assistant.delta" => self.assistant.push_str(
                payload
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            "tool.started" => self.tool_calls += 1,
            "session.updated" => {
                if let Some(id) = payload.get("session_id").and_then(Value::as_str) {
                    self.session_id = Some(id.to_string());
                }
            }
            "turn.completed" => self.terminal = Some("completed".into()),
            "turn.cancelled" => self.terminal = Some("cancelled".into()),
            "error" => {
                self.terminal = Some("failed".into());
                self.error = Some(
                    payload
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("出错了")
                        .into(),
                );
            }
            // Kept for compatibility with the one-shot CLI stream. The HTTP
            // runtime normally reports the same data through session.updated
            // and turn.completed.
            "print.result" => {
                self.result = Some(payload.clone());
                if let Some(id) = payload.get("session_id").and_then(Value::as_str) {
                    self.session_id = Some(id.to_string());
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationState {
    cursor: u64,
    messages: Vec<Value>,
    revision: u64,
    session_id: Option<String>,
    active: Option<TurnSnapshot>,
}

#[derive(Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum AgentEvent {
    Conversation {
        cursor: u64,
        revision: u64,
        session_id: Option<String>,
    },
    Started {
        cursor: u64,
        revision: u64,
        turn: TurnSnapshot,
    },
    Envelope {
        cursor: u64,
        revision: u64,
        request_id: String,
        sequence: u64,
        envelope: Value,
    },
    Finished {
        cursor: u64,
        revision: u64,
        turn: TurnSnapshot,
        exit_code: i32,
        cancelled: bool,
    },
}

struct ServerControl {
    child: Mutex<Option<Child>>,
    base_url: String,
    auth_token: String,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
}

impl ServerControl {
    fn stop(&self) {
        if let Ok(mut slot) = self.child.lock() {
            if let Some(child) = slot.as_mut() {
                request_graceful_shutdown(child);
                let deadline = std::time::Instant::now() + SERVER_STOP_GRACE;
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) if std::time::Instant::now() < deadline => {
                            thread::sleep(SERVER_POLL_INTERVAL);
                        }
                        Ok(None) | Err(_) => {
                            // The runtime did not flush and exit in time. Keep
                            // the hard-kill fallback so a stopped conversation
                            // cannot leave an orphan server behind.
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                    }
                }
            }
            slot.take();
        }
    }

    fn is_running(&self) -> bool {
        let Ok(mut slot) = self.child.lock() else {
            return false;
        };
        let Some(child) = slot.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) | Err(_) => {
                slot.take();
                false
            }
        }
    }

    fn wait_ready(&self) -> Result<(), String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(300))
            .build()
            .map_err(|error| format!("创建 deepcode 连接失败：{error}"))?;
        let deadline = std::time::Instant::now() + SERVER_START_TIMEOUT;
        let health = format!("{}/health", self.base_url);
        while std::time::Instant::now() < deadline {
            if !self.is_running() {
                return Err(format!("deepcode runtime 启动失败：{}", self.stderr()));
            }
            if let Ok(response) = client.get(&health).bearer_auth(&self.auth_token).send() {
                if response.status().is_success() {
                    return Ok(());
                }
            }
            thread::sleep(SERVER_POLL_INTERVAL);
        }
        Err(format!("等待 deepcode runtime 就绪超时：{}", self.stderr()))
    }

    fn stderr(&self) -> String {
        self.stderr_tail
            .lock()
            .map(|tail| String::from_utf8_lossy(&tail).trim().to_string())
            .unwrap_or_default()
    }
}

#[cfg(unix)]
fn request_graceful_shutdown(child: &Child) {
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SIGTERM lets deepcode flush its session store and stop its runtime
        // before the hard-kill fallback above is considered.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

#[cfg(windows)]
fn request_graceful_shutdown(child: &Child) {
    // A GUI-launched child has no console to receive Ctrl+C. taskkill without
    // /F asks Windows to close the process tree; the bounded wait and kill
    // fallback below still guarantee cleanup if it refuses.
    let mut command = std::process::Command::new("taskkill");
    command.args(["/PID", &child.id().to_string(), "/T"]);
    super::resolver::hide_console(&mut command);
    let _ = command.status();
}

#[cfg(not(any(unix, windows)))]
fn request_graceful_shutdown(_child: &Child) {
    // The bounded wait gives a naturally exiting process a chance to finish;
    // the fallback below guarantees cleanup when the platform has no portable
    // graceful process signal.
}

impl Drop for ServerControl {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct ProcessControl {
    cancelled: AtomicBool,
    // The child field remains for the low-level cancellation tests and for
    // future transports. HTTP turns cancel by terminating the runtime server.
    child: Mutex<Option<Child>>,
    server: Mutex<Option<Arc<ServerControl>>>,
}

impl ProcessControl {
    fn attach_server(&self, server: Arc<ServerControl>) {
        if let Ok(mut slot) = self.server.lock() {
            *slot = Some(server);
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.child.lock() {
            if let Some(child) = slot.as_mut() {
                let _ = child.kill();
            }
        }
        if let Ok(slot) = self.server.lock() {
            if let Some(server) = slot.as_ref() {
                server.stop();
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    fn wait_for_exit(&self) -> i32 {
        loop {
            let code = {
                let Ok(mut slot) = self.child.lock() else {
                    return -1;
                };
                let Some(child) = slot.as_mut() else {
                    return -1;
                };
                match child.try_wait() {
                    Ok(Some(status)) => {
                        slot.take();
                        Some(status.code().unwrap_or(-1))
                    }
                    Ok(None) => None,
                    Err(_) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        slot.take();
                        Some(-1)
                    }
                }
            };
            if let Some(code) = code {
                return code;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

struct ActiveTurn {
    history: Vec<Value>,
    snapshot: TurnSnapshot,
    control: Arc<ProcessControl>,
}

#[derive(Default)]
struct RuntimeState {
    cursor: u64,
    revision: u64,
    active: Option<ActiveTurn>,
    server: Option<Arc<ServerControl>>,
}

impl RuntimeState {
    fn check_start(&self, expected_revision: u64) -> Result<(), String> {
        if self.revision != expected_revision {
            return Err("对话已切换，请重新发送".into());
        }
        if self.active.is_some() {
            return Err("上一轮还没结束，请稍等或先停止".into());
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct AgentRuntime(Mutex<RuntimeState>);

#[tauri::command]
pub(crate) fn conversation_state(
    app: tauri::AppHandle,
    runtime: tauri::State<AgentRuntime>,
) -> Result<ConversationState, String> {
    let state = runtime.0.lock().map_err(|e| e.to_string())?;
    let session_id = initialize_current(&app)?;
    let messages = match &state.active {
        Some(active) => active.history.clone(),
        None => match &session_id {
            Some(id) => load_history(app.clone(), Some(80), Some(id.clone()))?,
            None => Vec::new(),
        },
    };
    Ok(ConversationState {
        cursor: state.cursor,
        messages,
        revision: state.revision,
        session_id,
        active: state.active.as_ref().map(|turn| turn.snapshot.clone()),
    })
}

pub(crate) fn select_conversation(app: &tauri::AppHandle, id: Option<&str>) -> Result<(), String> {
    let runtime = app.state::<AgentRuntime>();
    let (active, server, emitted) = {
        let mut state = runtime.0.lock().map_err(|e| e.to_string())?;
        write_current(app, id)?;
        state.revision += 1;
        state.cursor += 1;
        let active = state.active.take();
        let server = state.server.take();
        // Publish while holding the lock so a new turn cannot overtake the selection.
        let emitted = app.emit(
            "agent-event",
            AgentEvent::Conversation {
                cursor: state.cursor,
                revision: state.revision,
                session_id: id.map(str::to_owned),
            },
        );
        (active, server, emitted)
    };
    if let Some(active) = active {
        active.control.cancel();
    }
    if let Some(server) = server {
        server.stop();
    }
    emitted.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn cancel_turn(app: tauri::AppHandle, request_id: String) -> Result<(), String> {
    stop(&app, Some(&request_id))
}

pub(crate) fn stop(app: &tauri::AppHandle, request_id: Option<&str>) -> Result<(), String> {
    let runtime = app.state::<AgentRuntime>();
    let (active, server) = {
        let mut state = runtime.0.lock().map_err(|e| e.to_string())?;
        if state
            .active
            .as_ref()
            .is_some_and(|active| request_id.is_some_and(|id| id != active.snapshot.request_id))
        {
            return Err("这一轮已经结束".into());
        }
        (state.active.take(), state.server.take())
    };

    if let Some(mut active) = active {
        active.control.cancel();
        active.snapshot.sequence += 1;
        active.snapshot.terminal = Some("cancelled".into());
        let runtime = app.state::<AgentRuntime>();
        let mut state = runtime.0.lock().map_err(|e| e.to_string())?;
        state.cursor += 1;
        app.emit(
            "agent-event",
            AgentEvent::Finished {
                cursor: state.cursor,
                revision: state.revision,
                turn: active.snapshot,
                exit_code: 0,
                cancelled: true,
            },
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(server) = server {
        server.stop();
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn ask_deepcode(
    app: tauri::AppHandle,
    prompt: String,
    request_id: String,
    expected_revision: u64,
) -> Result<(), String> {
    if prompt.trim().is_empty() {
        return Err("请输入消息".into());
    }
    if request_id.is_empty() || request_id.len() > 128 {
        return Err("无效的回合标识".into());
    }
    let bin = resolve_agent_bin(&app).ok_or_else(missing_agent_message)?;
    let control = Arc::new(ProcessControl::default());
    let (revision, session_id) = {
        let runtime = app.state::<AgentRuntime>();
        let mut state = runtime.0.lock().map_err(|e| e.to_string())?;
        state.check_start(expected_revision)?;
        let session_id = initialize_current(&app)?;
        let history = match &session_id {
            Some(id) => load_history(app.clone(), Some(80), Some(id.clone()))?,
            None => Vec::new(),
        };
        let snapshot = TurnSnapshot {
            request_id: request_id.clone(),
            session_id: session_id.clone(),
            prompt: prompt.clone(),
            sequence: 0,
            assistant: String::new(),
            tool_calls: 0,
            terminal: None,
            result: None,
            error: None,
        };
        state.cursor += 1;
        state.active = Some(ActiveTurn {
            history,
            snapshot: snapshot.clone(),
            control: Arc::clone(&control),
        });
        if let Err(error) = app.emit(
            "agent-event",
            AgentEvent::Started {
                cursor: state.cursor,
                revision: state.revision,
                turn: snapshot,
            },
        ) {
            state.active = None;
            return Err(error.to_string());
        }
        (state.revision, session_id)
    };

    let worker_app = app.clone();
    let worker_id = request_id.clone();
    let worker_prompt = prompt.clone();
    let worker_control = Arc::clone(&control);
    let result = tauri::async_runtime::spawn_blocking(move || {
        run_turn(
            &worker_app,
            &worker_id,
            &worker_prompt,
            session_id.as_deref(),
            revision,
            &bin,
            &worker_control,
        )
    })
    .await;
    if let Err(error) = result {
        control.cancel();
        finish(
            &app,
            &request_id,
            revision,
            -1,
            Some(format!("deepcode runtime 异常：{error}")),
        );
        return Err(error.to_string());
    }
    Ok(())
}

fn run_turn(
    app: &tauri::AppHandle,
    request_id: &str,
    prompt: &str,
    session_id: Option<&str>,
    revision: u64,
    bin: &std::path::Path,
    control: &ProcessControl,
) {
    if control.is_cancelled() {
        return;
    }
    let server = match ensure_server(app, request_id, revision, session_id, bin, control) {
        Ok(server) => server,
        Err(error) => {
            finish(app, request_id, revision, -1, Some(error));
            return;
        }
    };
    if control.is_cancelled() {
        return;
    }

    let client = match reqwest::blocking::Client::builder().build() {
        Ok(client) => client,
        Err(error) => {
            finish(
                app,
                request_id,
                revision,
                -1,
                Some(format!("创建 deepcode 请求失败：{error}")),
            );
            return;
        }
    };
    let endpoint = format!("{}/v1/prompt", server.base_url);
    let response = match client
        .post(endpoint)
        .bearer_auth(&server.auth_token)
        .json(&serde_json::json!({ "prompt": prompt }))
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            if !control.is_cancelled() {
                finish(
                    app,
                    request_id,
                    revision,
                    -1,
                    Some(format!("连接 deepcode runtime 失败：{error}")),
                );
            }
            return;
        }
    };
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        finish(
            app,
            request_id,
            revision,
            -1,
            Some(format!("deepcode runtime 返回 {status}: {}", body.trim())),
        );
        return;
    }

    let mut read_error = None;
    let mut terminal = None;
    for line in BufReader::new(response).lines() {
        match line {
            Ok(line) => match parse_sse_data(&line) {
                Ok(Some(envelope)) => {
                    if let Some(kind) = terminal_kind(&envelope) {
                        terminal = Some(kind);
                    }
                    publish(app, request_id, revision, envelope);
                }
                Ok(None) => {}
                Err(error) => {
                    read_error = Some(format!("解析 deepcode runtime 事件失败：{error}"));
                    break;
                }
            },
            Err(error) => {
                read_error = Some(format!("读取 deepcode runtime 事件失败：{error}"));
                break;
            }
        }
    }
    if !control.is_cancelled() {
        let (code, error) = match terminal {
            Some("completed") | Some("cancelled") => (0, read_error),
            Some("error") => (-1, read_error),
            Some(_) | None => (
                -1,
                read_error.or_else(|| Some("deepcode runtime 流在终止事件前结束".into())),
            ),
        };
        finish(app, request_id, revision, code, error);
    }
}

fn parse_sse_data(line: &str) -> Result<Option<Value>, String> {
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(None);
    };
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }
    let value = serde_json::from_str::<Value>(data).map_err(|error| error.to_string())?;
    Ok(value.get("item").is_some().then_some(value))
}

fn terminal_kind(envelope: &Value) -> Option<&'static str> {
    let item = envelope.get("item").unwrap_or(envelope);
    match item.get("kind").and_then(Value::as_str) {
        Some("turn.completed") => Some("completed"),
        Some("turn.cancelled") => Some("cancelled"),
        Some("error") => Some("error"),
        _ => None,
    }
}

fn ensure_server(
    app: &tauri::AppHandle,
    request_id: &str,
    revision: u64,
    session_id: Option<&str>,
    bin: &std::path::Path,
    control: &ProcessControl,
) -> Result<Arc<ServerControl>, String> {
    let runtime = app.state::<AgentRuntime>();
    {
        let state = runtime.0.lock().map_err(|e| e.to_string())?;
        if state.revision != revision
            || state
                .active
                .as_ref()
                .is_none_or(|active| active.snapshot.request_id != request_id)
        {
            return Err("对话已切换，请重新发送".into());
        }
        if let Some(server) = state.server.as_ref().filter(|server| server.is_running()) {
            control.attach_server(Arc::clone(server));
            return Ok(Arc::clone(server));
        }
    }

    let server = spawn_server(app, session_id, bin)?;
    if control.is_cancelled() {
        server.stop();
        return Err("这一轮已取消".into());
    }
    let mut state = runtime.0.lock().map_err(|e| e.to_string())?;
    if state.revision != revision
        || state
            .active
            .as_ref()
            .is_none_or(|active| active.snapshot.request_id != request_id)
    {
        server.stop();
        return Err("对话已切换，请重新发送".into());
    }
    state.server = Some(Arc::clone(&server));
    control.attach_server(Arc::clone(&server));
    Ok(server)
}

fn spawn_server(
    app: &tauri::AppHandle,
    session_id: Option<&str>,
    bin: &std::path::Path,
) -> Result<Arc<ServerControl>, String> {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("为 deepcode runtime 分配端口失败：{error}"))?
        .local_addr()
        .map_err(|error| format!("读取 deepcode runtime 端口失败：{error}"))?
        .port();
    let token = format!(
        "deep-code-pet-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let mut command = agent_command(bin)?;
    command
        .args([
            "serve",
            "--http",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--auth-token",
            &token,
        ])
        .env("PATH", child_path(bin))
        .current_dir(agent_workspace(app))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(id) = session_id {
        command.arg("--resume").arg(id);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 deepcode runtime 失败：{error}"))?;
    let stderr_tail = Arc::new(Mutex::new(Vec::new()));
    if let Some(stderr) = child.stderr.take() {
        let tail = Arc::clone(&stderr_tail);
        thread::spawn(move || drain_tail(stderr, tail));
    }
    if let Some(stdout) = child.stdout.take() {
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                line.clear();
            }
        });
    }
    let server = Arc::new(ServerControl {
        child: Mutex::new(Some(child)),
        base_url: format!("http://127.0.0.1:{port}"),
        auth_token: token,
        stderr_tail,
    });
    if let Err(error) = server.wait_ready() {
        server.stop();
        return Err(error);
    }
    Ok(server)
}

fn drain_tail(mut reader: impl Read, tail: Arc<Mutex<Vec<u8>>>) {
    let mut buffer = [0u8; 1024];
    loop {
        let Ok(len) = reader.read(&mut buffer) else {
            break;
        };
        if len == 0 {
            break;
        }
        if let Ok(mut output) = tail.lock() {
            output.extend_from_slice(&buffer[..len]);
            if output.len() > 4096 {
                let keep_from = output.len() - 4096;
                output.drain(..keep_from);
            }
        }
    }
}

fn publish(app: &tauri::AppHandle, request_id: &str, revision: u64, envelope: Value) {
    let runtime = app.state::<AgentRuntime>();
    if let Ok(mut state) = runtime.0.lock() {
        if state.revision != revision {
            return;
        }
        state.cursor += 1;
        let cursor = state.cursor;
        if let Some(active) = state
            .active
            .as_mut()
            .filter(|active| active.snapshot.request_id == request_id)
        {
            active.snapshot.record(&envelope);
            let _ = app.emit(
                "agent-event",
                AgentEvent::Envelope {
                    cursor,
                    revision,
                    request_id: request_id.into(),
                    sequence: active.snapshot.sequence,
                    envelope,
                },
            );
        }
    };
}

fn finish(
    app: &tauri::AppHandle,
    request_id: &str,
    revision: u64,
    code: i32,
    error: Option<String>,
) {
    let runtime = app.state::<AgentRuntime>();
    if let Ok(mut state) = runtime.0.lock() {
        if state.revision != revision
            || state
                .active
                .as_ref()
                .is_none_or(|active| active.snapshot.request_id != request_id)
        {
            return;
        }
        let Some(mut active) = state.active.take() else {
            return;
        };
        state.cursor += 1;
        active.snapshot.sequence += 1;
        let mut final_code = code;
        let mut final_error = error;
        if final_error.is_some() && final_code == 0 {
            final_code = -1;
        }
        if final_code == 0 && active.snapshot.terminal.as_deref() == Some("completed") {
            match active.snapshot.session_id.clone() {
                Some(id) => {
                    if let Err(error) = verify_session_persisted(app, &id) {
                        final_code = -1;
                        final_error = Some(format!("deepcode session 保存失败：{error}"));
                    } else if let Err(error) = write_current(app, Some(&id)) {
                        final_code = -1;
                        final_error = Some(format!("保存当前 deepcode session 失败：{error}"));
                    }
                }
                None => {
                    final_code = -1;
                    final_error = Some("deepcode 没有返回已保存的 session ID".into());
                }
            }
        }
        if let Some(error) = final_error {
            active.snapshot.error = Some(error);
        }
        if state
            .server
            .as_ref()
            .is_some_and(|server| !server.is_running())
        {
            state.server.take();
        }
        let _ = app.emit(
            "agent-event",
            AgentEvent::Finished {
                cursor: state.cursor,
                revision,
                turn: active.snapshot,
                exit_code: final_code,
                cancelled: false,
            },
        );
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn rejects_a_prompt_queued_before_the_conversation_changed() {
        let state = RuntimeState {
            revision: 2,
            ..RuntimeState::default()
        };
        assert_eq!(state.check_start(1), Err("对话已切换，请重新发送".into()));
        assert!(state.check_start(2).is_ok());
    }

    #[test]
    fn snapshot_tracks_stream_and_session_update() {
        let mut turn = TurnSnapshot {
            request_id: "a".into(),
            session_id: None,
            prompt: "hi".into(),
            sequence: 0,
            assistant: String::new(),
            tool_calls: 0,
            terminal: None,
            result: None,
            error: None,
        };
        turn.record(&serde_json::json!({
            "item":{"kind":"session.updated","payload":{"session_id":"session-a"}}
        }));
        turn.record(&serde_json::json!({
            "item":{"kind":"assistant.delta","payload":{"text":"hello"}}
        }));
        turn.record(&serde_json::json!({
            "item":{"kind":"turn.completed","payload":{}}
        }));
        assert_eq!(turn.assistant, "hello");
        assert_eq!(turn.sequence, 3);
        assert_eq!(turn.session_id.as_deref(), Some("session-a"));
        assert_eq!(turn.terminal.as_deref(), Some("completed"));
    }

    #[test]
    fn parses_only_runtime_sse_data_lines() {
        assert_eq!(parse_sse_data("event: assistant.delta").unwrap(), None);
        assert_eq!(parse_sse_data("data: [DONE]").unwrap(), None);
        assert!(parse_sse_data("data: {not json}").is_err());
        let value =
            parse_sse_data(r#"data: {"item":{"kind":"assistant.delta","payload":{"text":"hi"}}}"#)
                .unwrap()
                .unwrap();
        assert_eq!(value["item"]["kind"], "assistant.delta");
    }

    #[test]
    fn only_explicit_terminal_events_complete_a_stream() {
        assert_eq!(
            terminal_kind(&serde_json::json!({
                "item": {"kind": "turn.completed", "payload": {}}
            })),
            Some("completed")
        );
        assert_eq!(
            terminal_kind(&serde_json::json!({
                "item": {"kind": "turn.cancelled", "payload": {}}
            })),
            Some("cancelled")
        );
        assert_eq!(
            terminal_kind(&serde_json::json!({
                "item": {"kind": "assistant.delta", "payload": {"text": "partial"}}
            })),
            None
        );
    }

    #[test]
    fn events_serialize_to_the_frontend_contract() {
        let event = AgentEvent::Envelope {
            cursor: 12,
            revision: 2,
            request_id: "request-a".into(),
            sequence: 3,
            envelope: serde_json::json!({"item":{"kind":"assistant.delta","payload":{"text":"hello"}}}),
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "envelope");
        assert_eq!(value["requestId"], "request-a");
        assert_eq!(value["cursor"], 12);
        assert!(value.get("request_id").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_terminates_and_reaps_a_child() {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let control = ProcessControl {
            cancelled: AtomicBool::new(false),
            child: Mutex::new(Some(child)),
            server: Mutex::new(None),
        };
        control.cancel();
        let mut child = control.child.lock().unwrap().take().unwrap();
        assert!(!child.wait().unwrap().success());
        assert!(control.cancelled.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_remains_cancellable_after_closing_stdout() {
        use std::{sync::mpsc, time::Duration};
        let mut child = Command::new("sh")
            .args(["-c", "exec 1>&-; exec sleep 30"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut line = String::new();
        assert_eq!(
            BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap(),
            0
        );
        let control = Arc::new(ProcessControl {
            cancelled: AtomicBool::new(false),
            child: Mutex::new(Some(child)),
            server: Mutex::new(None),
        });
        let waiting = Arc::clone(&control);
        let (started, ready) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            finished.send(waiting.wait_for_exit()).unwrap();
        });
        ready.recv().unwrap();
        std::thread::sleep(Duration::from_millis(30));
        control.cancel();
        let code = result.recv_timeout(Duration::from_secs(2));
        if code.is_err() {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
        worker.join().unwrap();
        assert_eq!(code.unwrap(), -1);
    }
}
