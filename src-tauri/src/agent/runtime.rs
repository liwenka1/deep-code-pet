//! Owns conversation revisions and child processes. Webviews only project this state.
use serde::Serialize;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager};

use super::{
    persona::agent_workspace,
    resolver::{agent_command, child_path, missing_agent_message, resolve_agent_bin},
};
use crate::{
    conversations::{initialize_current, load_history, session_file, write_current},
    settings::resolve_model,
};

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
        let payload = &item["payload"];
        match item["kind"].as_str().unwrap_or_default() {
            "assistant.delta" => self
                .assistant
                .push_str(payload["text"].as_str().unwrap_or_default()),
            "tool.started" => self.tool_calls += 1,
            "turn.completed" => self.terminal = Some("completed".into()),
            "turn.cancelled" => self.terminal = Some("cancelled".into()),
            "error" => {
                self.terminal = Some("failed".into());
                self.error = Some(payload["message"].as_str().unwrap_or("出错了").into());
            }
            "print.result" => self.result = Some(payload.clone()),
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

#[derive(Default)]
struct ProcessControl {
    cancelled: AtomicBool,
    child: Mutex<Option<Child>>,
}

impl ProcessControl {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.child.lock() {
            if let Some(child) = slot.as_mut() {
                let _ = child.kill();
            }
        }
    }

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
            // Keep the handle available to cancel(), including after stdout closes.
            std::thread::sleep(std::time::Duration::from_millis(10));
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
    let (active, emitted) = {
        let mut state = runtime.0.lock().map_err(|e| e.to_string())?;
        write_current(app, id)?;
        state.revision += 1;
        state.cursor += 1;
        let active = state.active.take();
        // Publish while holding the lock so a new turn cannot overtake the selection.
        let emitted = app.emit(
            "agent-event",
            AgentEvent::Conversation {
                cursor: state.cursor,
                revision: state.revision,
                session_id: id.map(str::to_owned),
            },
        );
        (active, emitted)
    };
    if let Some(active) = active {
        active.control.cancel();
    }
    emitted.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn cancel_turn(app: tauri::AppHandle, request_id: String) -> Result<(), String> {
    stop(&app, Some(&request_id))
}

pub(crate) fn stop(app: &tauri::AppHandle, request_id: Option<&str>) -> Result<(), String> {
    let runtime = app.state::<AgentRuntime>();
    let mut state = runtime.0.lock().map_err(|e| e.to_string())?;
    if state
        .active
        .as_ref()
        .is_some_and(|active| request_id.is_some_and(|id| id != active.snapshot.request_id))
    {
        return Err("这一轮已经结束".into());
    }
    if let Some(mut active) = state.active.take() {
        active.control.cancel();
        state.cursor += 1;
        active.snapshot.sequence += 1;
        active.snapshot.terminal = Some("cancelled".into());
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
    let mut command = agent_command(&bin)?;
    command
        .arg("-p")
        .arg(&prompt)
        .args(["--output-format", "stream-json"])
        .env("DEEP_CODE_MODEL", resolve_model(&app))
        .env("PATH", child_path(&bin))
        .current_dir(agent_workspace(&app))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let control = Arc::new(ProcessControl::default());
    let revision = {
        let runtime = app.state::<AgentRuntime>();
        let mut state = runtime.0.lock().map_err(|e| e.to_string())?;
        state.check_start(expected_revision)?;
        let session_id = initialize_current(&app)?;
        match &session_id {
            Some(id) => {
                command.arg("--resume").arg(id);
            }
            None => {
                command.arg("--new");
            }
        }
        let history = match &session_id {
            Some(id) => load_history(app.clone(), Some(80), Some(id.clone()))?,
            None => Vec::new(),
        };
        let snapshot = TurnSnapshot {
            request_id: request_id.clone(),
            session_id,
            prompt,
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
        state.revision
    };
    let worker_app = app.clone();
    let worker_id = request_id.clone();
    let worker_control = Arc::clone(&control);
    // The command resolves only after the child has exited and the final event was sent.
    let result = tauri::async_runtime::spawn_blocking(move || {
        run_process(&worker_app, &worker_id, revision, command, &worker_control)
    })
    .await;
    if let Err(error) = result {
        control.cancel();
        finish(
            &app,
            &request_id,
            revision,
            -1,
            Some(format!("聊天任务异常：{error}")),
        );
        return Err(error.to_string());
    }
    Ok(())
}

fn run_process(
    app: &tauri::AppHandle,
    request_id: &str,
    revision: u64,
    mut command: Command,
    control: &ProcessControl,
) {
    if control.cancelled.load(Ordering::SeqCst) {
        return;
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            finish(
                app,
                request_id,
                revision,
                -1,
                Some(format!("启动 deepcode 失败：{error}")),
            );
            return;
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    if let Ok(mut slot) = control.child.lock() {
        *slot = Some(child);
    } else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    }
    if control.cancelled.load(Ordering::SeqCst) {
        control.cancel();
    }
    // Drain stderr independently, retaining only a bounded tail for useful GUI errors.
    let stderr_thread = std::thread::spawn(move || {
        let mut tail = Vec::new();
        if let Some(mut stderr) = stderr {
            let mut buf = [0u8; 1024];
            while let Ok(len) = stderr.read(&mut buf) {
                if len == 0 {
                    break;
                }
                tail.extend_from_slice(&buf[..len]);
                if tail.len() > 4096 {
                    tail.drain(..tail.len() - 4096);
                }
            }
        }
        String::from_utf8_lossy(&tail).trim().to_string()
    });
    let mut read_error = None;
    if let Some(stdout) = stdout {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if let Ok(envelope) = serde_json::from_str::<Value>(&line) {
                        if envelope.get("item").is_some() || envelope.get("kind").is_some() {
                            publish(app, request_id, revision, envelope);
                        }
                    }
                }
                Err(error) => {
                    read_error = Some(format!("读取回复失败：{error}"));
                    control.cancel();
                    break;
                }
            }
        }
    }
    let code = control.wait_for_exit();
    let stderr = stderr_thread.join().unwrap_or_default();
    let error = read_error.or_else(|| (code != 0 && !stderr.is_empty()).then_some(stderr));
    finish(app, request_id, revision, code, error);
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
        if let Some(error) = error {
            active.snapshot.error = Some(error);
        }
        let result_id = active
            .snapshot
            .result
            .as_ref()
            .and_then(|report| report["session_id"].as_str());
        if let Some(id) = result_id {
            match session_file(app, id) {
                Ok(path) if path.is_file() => {
                    if let Err(error) = write_current(app, Some(id)) {
                        active.snapshot.error = Some(error);
                    } else {
                        active.snapshot.session_id = Some(id.into());
                    }
                }
                _ => {
                    active.snapshot.error = Some("回复已完成，但会话记录未能保存".into());
                }
            }
        }
        let _ = app.emit(
            "agent-event",
            AgentEvent::Finished {
                cursor: state.cursor,
                revision,
                turn: active.snapshot,
                exit_code: code,
                cancelled: false,
            },
        );
    };
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn snapshot_tracks_stream_and_exact_result_session() {
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
        turn.record(
            &serde_json::json!({"item":{"kind":"assistant.delta","payload":{"text":"hello"}}}),
        );
        turn.record(&serde_json::json!({"item":{"kind":"print.result","payload":{"session_id":"session-a","status":"finished"}}}));
        assert_eq!(turn.assistant, "hello");
        assert_eq!(turn.sequence, 2);
        assert_eq!(turn.result.as_ref().unwrap()["session_id"], "session-a");
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
