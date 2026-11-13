//! 任务队列（Rust 端真实队列）：状态机、并发调度、取消 / 重试 / 仅重试下载、持久化与重启恢复。
//!
//! 状态流转：
//! ```text
//! queued ──start──▶ generating ──generated──▶ downloading ──downloaded──▶ succeeded
//!   │                  │  └─fail──▶ failed            └─download_failed──▶ download_failed
//!   └──────cancel──────┴──────────────cancel───────────────▶ cancelled
//! failed / cancelled / download_failed ──retry──▶ queued
//! download_failed ──retry_download──▶ downloading（沿用结果 URL，不重新扣费）
//! ```
//! 每次状态变化写入 tasks.json，并通过 `task-updated` / `task-removed` 事件推给前端。

use crate::api::{self, ApiContext, GenParams, GenRequest, Model, Route, async_task, gpt_image};
use crate::download;
use crate::images;
use crate::settings::{Settings, build_client, write_atomic};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const EVENT_UPDATED: &str = "task-updated";
pub const EVENT_REMOVED: &str = "task-removed";
pub const EVENT_LOG: &str = "queue-log";

/// API 请求超时：GPT 同步出图实测约 40 秒，给 10 分钟
const API_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// 文生图
    Text,
    /// 图生图
    Image,
    /// 继续修改（以已有结果图为参考）
    Edit,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Generating,
    Downloading,
    Succeeded,
    Failed,
    DownloadFailed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_active(self) -> bool {
        matches!(self, TaskStatus::Queued | TaskStatus::Generating | TaskStatus::Downloading)
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    /// 同一次「加入队列」展开出的任务共用一个批次 id
    pub batch_id: String,
    /// 批内序号，从 1 开始，用于文件名
    pub seq: u32,
    /// 继续修改时，来源任务的 id
    pub parent_id: Option<String>,
    pub mode: Mode,
    pub model: Model,
    pub prompt: String,
    pub params: GenParams,
    /// 参考图本地路径
    pub ref_images: Vec<String>,
    pub status: TaskStatus,
    /// 0–100，Nano 轮询进度
    pub progress: u8,
    /// Nano 异步任务 id（重启后据此继续轮询）
    pub remote_id: Option<String>,
    /// 生成成功后的结果 URL（约 2 小时有效）
    pub result_urls: Vec<String>,
    /// 已下载到本地的文件
    pub files: Vec<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub attempts: u32,
    /// 是否显示在队列里；「清空已结束」后成功的任务只留在图库
    #[serde(default = "default_true")]
    pub in_queue: bool,
}

/// 重启后需要继续的工作。
#[derive(Debug, PartialEq)]
pub enum Recovery {
    None,
    Poll(String),
    Download,
}

type Transition = Result<(), String>;

fn expect(task: &Task, allowed: &[TaskStatus], action: &str) -> Transition {
    if allowed.contains(&task.status) {
        Ok(())
    } else {
        Err(format!("任务当前状态为 {:?}，不能{action}", task.status))
    }
}

impl Task {
    pub fn start(&mut self, now: i64) -> Transition {
        expect(self, &[TaskStatus::Queued], "开始生成")?;
        self.status = TaskStatus::Generating;
        self.progress = 0;
        self.error = None;
        self.started_at = Some(now);
        self.attempts += 1;
        Ok(())
    }

    pub fn set_remote_id(&mut self, id: String) -> Transition {
        expect(self, &[TaskStatus::Generating], "记录任务 id")?;
        self.remote_id = Some(id);
        Ok(())
    }

    pub fn set_progress(&mut self, progress: u8) -> Transition {
        expect(self, &[TaskStatus::Generating], "更新进度")?;
        self.progress = progress.min(100);
        Ok(())
    }

    pub fn generated(&mut self, urls: Vec<String>) -> Transition {
        expect(self, &[TaskStatus::Generating], "进入下载")?;
        self.result_urls = urls;
        self.status = TaskStatus::Downloading;
        self.progress = 100;
        Ok(())
    }

    pub fn downloaded(&mut self, files: Vec<String>, now: i64) -> Transition {
        expect(self, &[TaskStatus::Downloading], "完成")?;
        self.files = files;
        self.status = TaskStatus::Succeeded;
        self.error = None;
        self.finished_at = Some(now);
        Ok(())
    }

    pub fn fail(&mut self, message: String, now: i64) -> Transition {
        expect(self, &[TaskStatus::Queued, TaskStatus::Generating], "标记失败")?;
        self.status = TaskStatus::Failed;
        self.error = Some(message);
        self.finished_at = Some(now);
        Ok(())
    }

    pub fn download_failed(&mut self, message: String, now: i64) -> Transition {
        expect(self, &[TaskStatus::Downloading], "标记下载失败")?;
        self.status = TaskStatus::DownloadFailed;
        self.error = Some(message);
        self.finished_at = Some(now);
        Ok(())
    }

    pub fn cancel(&mut self, now: i64) -> Transition {
        expect(self, &[TaskStatus::Queued, TaskStatus::Generating, TaskStatus::Downloading], "取消")?;
        self.status = TaskStatus::Cancelled;
        self.error = None;
        self.finished_at = Some(now);
        Ok(())
    }

    /// 失败重试：重新生成（会重新扣费）。
    pub fn reset_for_retry(&mut self) -> Transition {
        expect(self, &[TaskStatus::Failed, TaskStatus::Cancelled, TaskStatus::DownloadFailed], "重试")?;
        self.status = TaskStatus::Queued;
        self.progress = 0;
        self.remote_id = None;
        self.result_urls.clear();
        self.files.clear();
        self.error = None;
        self.started_at = None;
        self.finished_at = None;
        self.in_queue = true;
        Ok(())
    }

    /// 仅重试下载：沿用结果 URL，不重新扣费。
    pub fn retry_download(&mut self) -> Transition {
        expect(self, &[TaskStatus::DownloadFailed], "重试下载")?;
        if self.result_urls.is_empty() {
            return Err("没有可下载的结果地址，请改用「重试」".into());
        }
        self.status = TaskStatus::Downloading;
        self.error = None;
        self.finished_at = None;
        Ok(())
    }

    /// 重启恢复：生成中的异步任务（Nano，或带参考图的 GPT）有任务 id 则继续轮询；
    /// 下载中的继续下载；其余进行中的（同步 GPT、没拿到 id 的）标为失败，可重试。
    pub fn recover(&mut self, now: i64) -> Recovery {
        match self.status {
            TaskStatus::Generating => match &self.remote_id {
                Some(id) => Recovery::Poll(id.clone()),
                None => {
                    let _ = self.fail("应用重启，生成被中断，可点「重试」重新生成".into(), now);
                    Recovery::None
                }
            },
            TaskStatus::Downloading if !self.result_urls.is_empty() => Recovery::Download,
            TaskStatus::Downloading => {
                self.status = TaskStatus::Failed;
                self.error = Some("应用重启，下载被中断且没有结果地址，可点「重试」".into());
                self.finished_at = Some(now);
                Recovery::None
            }
            _ => Recovery::None,
        }
    }

    /// 日志里用的简短标识
    pub fn label(&self) -> String {
        let short: String = self.prompt.chars().take(16).collect();
        let more = if self.prompt.chars().count() > 16 { "…" } else { "" };
        format!("「{short}{more}」#{}", self.seq)
    }
}

/// 前端「加入队列」的请求。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueRequest {
    pub mode: Mode,
    pub model: Model,
    pub prompt: String,
    pub params: GenParams,
    #[serde(default)]
    pub ref_images: Vec<String>,
    pub count: u8,
    #[serde(default)]
    pub parent_id: Option<String>,
}

/// 校验请求并展开成 `count` 个独立任务。
pub fn expand_request(req: &EnqueueRequest, now: i64, mut next_id: impl FnMut() -> String) -> Result<Vec<Task>, String> {
    let prompt = req.prompt.trim();
    if prompt.is_empty() {
        return Err("请先填写提示词".into());
    }
    if matches!(req.mode, Mode::Image | Mode::Edit) && req.ref_images.is_empty() {
        return Err("图生图 / 继续修改需要至少一张参考图".into());
    }
    if req.mode == Mode::Text && !req.ref_images.is_empty() {
        return Err("文生图模式不带参考图".into());
    }
    let count = req.count.clamp(1, 4) as u32;
    let batch_id = next_id();
    Ok((1..=count)
        .map(|seq| Task {
            id: next_id(),
            batch_id: batch_id.clone(),
            seq,
            parent_id: req.parent_id.clone(),
            mode: req.mode,
            model: req.model,
            prompt: prompt.to_string(),
            params: req.params.clone(),
            ref_images: req.ref_images.clone(),
            status: TaskStatus::Queued,
            progress: 0,
            remote_id: None,
            result_urls: vec![],
            files: vec![],
            error: None,
            created_at: now,
            started_at: None,
            finished_at: None,
            attempts: 0,
            in_queue: true,
        })
        .collect())
}

/// 调度：在不超过并发上限的前提下，按入队顺序挑出要启动的排队任务。
pub fn pick_to_start(tasks: &[Task], running: usize, limit: usize) -> Vec<String> {
    let free = limit.saturating_sub(running);
    tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Queued)
        .take(free)
        .map(|t| t.id.clone())
        .collect()
}

#[derive(Serialize, Deserialize)]
struct TaskFile {
    version: u32,
    tasks: Vec<Task>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogEvent {
    level: &'static str,
    message: String,
}

enum Phase {
    Generate,
    Poll(String),
    Download,
}

struct Running {
    run_id: u64,
    handle: tauri::async_runtime::JoinHandle<()>,
}

struct State {
    tasks: Vec<Task>,
    running: HashMap<String, Running>,
}

pub struct Queue {
    app: AppHandle,
    settings: Arc<RwLock<Settings>>,
    tasks_path: PathBuf,
    default_output_dir: PathBuf,
    state: Mutex<State>,
    counter: AtomicU64,
}

pub fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

impl Queue {
    pub fn new(app: AppHandle, settings: Arc<RwLock<Settings>>, tasks_path: PathBuf, default_output_dir: PathBuf) -> Arc<Self> {
        let tasks = std::fs::read_to_string(&tasks_path)
            .ok()
            .and_then(|text| serde_json::from_str::<TaskFile>(&text).ok())
            .map(|f| f.tasks)
            .unwrap_or_default();
        Arc::new(Self {
            app,
            settings,
            tasks_path,
            default_output_dir,
            state: Mutex::new(State { tasks, running: HashMap::new() }),
            counter: AtomicU64::new(0),
        })
    }

    fn next_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{:x}{:04x}", now_ms(), n & 0xffff)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn output_dir(&self, settings: &Settings) -> PathBuf {
        let custom = settings.output_dir.trim();
        if custom.is_empty() { self.default_output_dir.clone() } else { PathBuf::from(custom) }
    }

    fn save(&self, st: &State) {
        let file = TaskFile { version: 1, tasks: st.tasks.clone() };
        if let Ok(text) = serde_json::to_string_pretty(&file)
            && let Err(e) = write_atomic(&self.tasks_path, text.as_bytes())
        {
            self.log("error", format!("保存队列失败：{e}"));
        }
    }

    fn emit(&self, task: &Task) {
        let _ = self.app.emit(EVENT_UPDATED, task);
    }

    pub fn log(&self, level: &'static str, message: String) {
        let _ = self.app.emit(EVENT_LOG, LogEvent { level, message });
    }

    pub fn list(&self) -> Vec<Task> {
        self.lock().tasks.clone()
    }

    fn snapshot(&self, id: &str) -> Option<Task> {
        self.lock().tasks.iter().find(|t| t.id == id).cloned()
    }

    /// 对任务应用一次状态变化；任务已被删除或状态不允许（例如已取消）时返回 None。
    fn update(&self, id: &str, persist: bool, f: impl FnOnce(&mut Task) -> Transition) -> Option<Task> {
        let mut st = self.lock();
        let task = st.tasks.iter_mut().find(|t| t.id == id)?;
        f(task).ok()?;
        let task = task.clone();
        self.emit(&task);
        if persist {
            self.save(&st);
        }
        Some(task)
    }

    /// 启动时调用：恢复中断的任务并开始调度。
    pub fn start(self: &Arc<Self>) {
        let mut st = self.lock();
        let now = now_ms();
        let mut resume = Vec::new();
        for task in st.tasks.iter_mut() {
            match task.recover(now) {
                Recovery::Poll(remote) => resume.push((task.id.clone(), Phase::Poll(remote))),
                Recovery::Download => resume.push((task.id.clone(), Phase::Download)),
                Recovery::None => {}
            }
        }
        let resumed = resume.len();
        for (id, phase) in resume {
            self.launch(&mut st, &id, phase);
        }
        self.save(&st);
        let queued = st.tasks.iter().filter(|t| t.status == TaskStatus::Queued).count();
        if resumed + queued > 0 {
            self.log("info", format!("已恢复上次的队列：继续 {resumed} 个进行中的任务，{queued} 个排队中。"));
        }
        self.pump_locked(&mut st);
    }

    fn launch(self: &Arc<Self>, st: &mut State, id: &str, phase: Phase) {
        let run_id = self.counter.fetch_add(1, Ordering::Relaxed);
        let queue = self.clone();
        let task_id = id.to_string();
        let handle = tauri::async_runtime::spawn(async move {
            queue.run(&task_id, phase).await;
            queue.on_worker_exit(&task_id, run_id);
        });
        st.running.insert(id.to_string(), Running { run_id, handle });
    }

    fn on_worker_exit(self: &Arc<Self>, id: &str, run_id: u64) {
        let mut st = self.lock();
        if st.running.get(id).is_some_and(|r| r.run_id == run_id) {
            st.running.remove(id);
        }
        self.pump_locked(&mut st);
    }

    /// 按并发上限启动排队任务。
    pub fn pump(self: &Arc<Self>) {
        let mut st = self.lock();
        self.pump_locked(&mut st);
    }

    fn pump_locked(self: &Arc<Self>, st: &mut State) {
        let limit = self.settings().concurrency.clamp(1, 5) as usize;
        let ids = pick_to_start(&st.tasks, st.running.len(), limit);
        if ids.is_empty() {
            return;
        }
        let now = now_ms();
        for id in ids {
            if let Some(task) = st.tasks.iter_mut().find(|t| t.id == id)
                && task.start(now).is_ok()
            {
                let task = task.clone();
                self.emit(&task);
                self.log("processing", format!("开始生成 {}（{}）", task.label(), task.model.id()));
                self.launch(st, &id, Phase::Generate);
            }
        }
        self.save(st);
    }

    pub fn enqueue(self: &Arc<Self>, req: EnqueueRequest) -> Result<Vec<Task>, String> {
        let settings = self.settings();
        if settings.api_key.trim().is_empty() {
            return Err("还没有填写 API Key，请先到「设置」里填写".into());
        }
        for path in &req.ref_images {
            if !Path::new(path).is_file() {
                return Err(format!("参考图不存在：{path}"));
            }
        }
        let tasks = expand_request(&req, now_ms(), || self.next_id())?;
        let mut st = self.lock();
        for task in &tasks {
            st.tasks.push(task.clone());
            self.emit(task);
        }
        self.save(&st);
        self.pump_locked(&mut st);
        Ok(tasks)
    }

    pub fn cancel(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let mut st = self.lock();
        let task = st.tasks.iter_mut().find(|t| t.id == id).ok_or("任务不存在")?;
        let was = task.status;
        task.cancel(now_ms())?;
        let task = task.clone();
        if let Some(r) = st.running.remove(id) {
            r.handle.abort();
        }
        self.emit(&task);
        self.log(
            "warning",
            if was == TaskStatus::Queued { format!("已取消排队中的 {}", task.label()) } else { format!("已取消 {}，结果将被丢弃", task.label()) },
        );
        self.save(&st);
        self.pump_locked(&mut st);
        Ok(())
    }

    pub fn retry(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let mut st = self.lock();
        let task = st.tasks.iter_mut().find(|t| t.id == id).ok_or("任务不存在")?;
        task.reset_for_retry()?;
        let task = task.clone();
        self.emit(&task);
        self.log("info", format!("重新排队 {}", task.label()));
        self.save(&st);
        self.pump_locked(&mut st);
        Ok(())
    }

    pub fn retry_download(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let mut st = self.lock();
        let task = st.tasks.iter_mut().find(|t| t.id == id).ok_or("任务不存在")?;
        task.retry_download()?;
        let task = task.clone();
        self.emit(&task);
        self.log("processing", format!("重新下载 {}（不重新生成，不扣费）", task.label()));
        self.launch(&mut st, id, Phase::Download);
        self.save(&st);
        Ok(())
    }

    /// 删除记录（已下载的图片文件保留在磁盘上）。进行中的任务先取消。
    pub fn delete(self: &Arc<Self>, ids: &[String]) {
        let mut st = self.lock();
        for id in ids {
            if let Some(r) = st.running.remove(id) {
                r.handle.abort();
            }
        }
        st.tasks.retain(|t| !ids.contains(&t.id));
        let _ = self.app.emit(EVENT_REMOVED, ids);
        self.save(&st);
        self.pump_locked(&mut st);
    }

    /// 清空已结束：失败 / 取消 / 下载失败的删除；已完成的从队列隐藏，仍保留在图库。
    pub fn clear_finished(self: &Arc<Self>) -> usize {
        let mut st = self.lock();
        let removed: Vec<String> = st
            .tasks
            .iter()
            .filter(|t| !t.status.is_active() && t.status != TaskStatus::Succeeded)
            .map(|t| t.id.clone())
            .collect();
        st.tasks.retain(|t| !removed.contains(&t.id));
        let mut hidden = Vec::new();
        for task in st.tasks.iter_mut().filter(|t| t.status == TaskStatus::Succeeded && t.in_queue) {
            task.in_queue = false;
            hidden.push(task.clone());
        }
        for task in &hidden {
            self.emit(task);
        }
        if !removed.is_empty() {
            let _ = self.app.emit(EVENT_REMOVED, &removed);
        }
        self.save(&st);
        removed.len() + hidden.len()
    }

    fn api_context(&self, settings: &Settings) -> Result<ApiContext, String> {
        let client = build_client(&settings.proxy, settings.proxy.for_api, API_TIMEOUT)?;
        Ok(ApiContext { base_url: settings.base_url().to_string(), api_key: settings.api_key.clone(), client })
    }

    async fn run(&self, id: &str, phase: Phase) {
        let Some(task) = self.snapshot(id) else { return };
        let settings = self.settings();
        if !matches!(phase, Phase::Download) {
            let result = match self.api_context(&settings) {
                Err(e) => Err(e),
                Ok(ctx) => match phase {
                    Phase::Poll(remote) => {
                        self.log("processing", format!("继续查询 {} 的生成进度", task.label()));
                        self.poll_nano(&ctx, id, &remote).await
                    }
                    _ => self.generate(&ctx, &task).await,
                },
            };
            match result {
                Ok(urls) => {
                    if self.update(id, true, |t| t.generated(urls)).is_none() {
                        return;
                    }
                    self.log("info", format!("{} 生成成功，开始下载", task.label()));
                }
                Err(message) => {
                    if self.update(id, true, |t| t.fail(message.clone(), now_ms())).is_some() {
                        self.log("error", format!("{} 失败：{message}", task.label()));
                    }
                    return;
                }
            }
        }
        self.download(id, &settings).await;
    }

    async fn generate(&self, ctx: &ApiContext, task: &Task) -> Result<Vec<String>, String> {
        let paths = task.ref_images.clone();
        let images = tauri::async_runtime::spawn_blocking(move || {
            paths.iter().map(|p| images::encode_reference(Path::new(p))).collect::<Result<Vec<_>, _>>()
        })
        .await
        .map_err(|e| e.to_string())??;
        let req = GenRequest { model: task.model, prompt: task.prompt.clone(), params: task.params.clone(), images };
        match api::route(&req) {
            Route::GptSync => gpt_image::generate(ctx, &req).await.map_err(|e| e.to_string()),
            Route::Async(body) => {
                let remote = async_task::submit(ctx, &body).await.map_err(|e| e.to_string())?;
                self.update(&task.id, true, |t| t.set_remote_id(remote.clone())).ok_or("任务已取消")?;
                self.poll_nano(ctx, &task.id, &remote).await
            }
        }
    }

    async fn poll_nano(&self, ctx: &ApiContext, id: &str, remote: &str) -> Result<Vec<String>, String> {
        let mut last = u8::MAX;
        async_task::poll(ctx, remote, |p| {
            if p != last {
                last = p;
                self.update(id, false, |t| t.set_progress(p));
            }
        })
        .await
        .map_err(|e| e.to_string())
    }

    async fn download(&self, id: &str, settings: &Settings) {
        let Some(task) = self.snapshot(id) else { return };
        let client = match build_client(&settings.proxy, settings.proxy.for_download, DOWNLOAD_TIMEOUT) {
            Ok(c) => c,
            Err(e) => {
                self.finish_download_failed(&task, e);
                return;
            }
        };
        let dir = self.output_dir(settings);
        let fallback = match task.model {
            Model::GptImage if !task.params.output_format.is_empty() => task.params.output_format.as_str(),
            _ => "png",
        };
        let mut files = Vec::new();
        for (i, url) in task.result_urls.iter().enumerate() {
            let name = download::build_file_name(task.created_at, task.model.id(), task.seq, i, &download::ext_from_url(url, fallback));
            let dest = match download::reserve_unique(&dir, &name) {
                Ok(p) => p,
                Err(e) => {
                    self.finish_download_failed(&task, format!("无法在输出目录创建文件：{e}"));
                    return;
                }
            };
            if let Err(e) = download::download(&client, url, &dest).await {
                let _ = std::fs::remove_file(&dest);
                for f in &files {
                    let _ = std::fs::remove_file(f);
                }
                self.finish_download_failed(&task, e);
                return;
            }
            files.push(dest.to_string_lossy().to_string());
        }
        let count = files.len();
        if self.update(id, true, |t| t.downloaded(files, now_ms())).is_some() {
            self.log("success", format!("{} 已完成，保存 {count} 张到 {}", task.label(), dir.display()));
        }
    }

    fn finish_download_failed(&self, task: &Task, message: String) {
        let text = format!("图片下载失败：{message}。结果已生成，可点「重试下载」（不重新扣费）");
        if self.update(&task.id, true, |t| t.download_failed(text.clone(), now_ms())).is_some() {
            self.log("error", format!("{} {text}", task.label()));
        }
    }

    /// 测试连接用的上下文（使用尚未保存的设置）。
    pub fn context_for(settings: &Settings) -> Result<ApiContext, String> {
        let client = build_client(&settings.proxy, settings.proxy.for_api, Duration::from_secs(30))?;
        Ok(ApiContext { base_url: settings.base_url().to_string(), api_key: settings.api_key.clone(), client })
    }
}

pub fn test_api(settings: &Settings) -> impl std::future::Future<Output = Result<String, String>> + use<> {
    let ctx = Queue::context_for(settings);
    async move {
        let ctx = ctx?;
        api::test_connection(&ctx).await.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(mode: Mode, refs: usize, count: u8) -> EnqueueRequest {
        EnqueueRequest {
            mode,
            model: Model::NanoBanana,
            prompt: "  水墨山水  ".into(),
            params: GenParams::default(),
            ref_images: (0..refs).map(|i| format!("/tmp/{i}.png")).collect(),
            count,
            parent_id: None,
        }
    }

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("id{n}")
        }
    }

    fn task() -> Task {
        expand_request(&request(Mode::Text, 0, 1), 1000, ids()).unwrap().remove(0)
    }

    #[test]
    fn expand_count_and_validation() {
        let tasks = expand_request(&request(Mode::Text, 0, 3), 1, ids()).unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks.iter().map(|t| t.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(tasks.iter().all(|t| t.batch_id == "id1" && t.prompt == "水墨山水" && t.status == TaskStatus::Queued));
        assert_eq!(expand_request(&request(Mode::Text, 0, 9), 1, ids()).unwrap().len(), 4);
        assert!(expand_request(&request(Mode::Image, 0, 1), 1, ids()).is_err());
        assert!(expand_request(&request(Mode::Text, 1, 1), 1, ids()).is_err());
        let mut empty = request(Mode::Text, 0, 1);
        empty.prompt = "   ".into();
        assert!(expand_request(&empty, 1, ids()).is_err());
        let mut edit = request(Mode::Edit, 1, 1);
        edit.parent_id = Some("p1".into());
        assert_eq!(expand_request(&edit, 1, ids()).unwrap()[0].parent_id.as_deref(), Some("p1"));
    }

    #[test]
    fn happy_path() {
        let mut t = task();
        t.start(1).unwrap();
        assert_eq!(t.status, TaskStatus::Generating);
        assert_eq!(t.attempts, 1);
        t.set_remote_id("r1".into()).unwrap();
        t.set_progress(40).unwrap();
        t.generated(vec!["https://x/a.png".into()]).unwrap();
        assert_eq!(t.status, TaskStatus::Downloading);
        t.downloaded(vec!["/out/a.png".into()], 2).unwrap();
        assert_eq!(t.status, TaskStatus::Succeeded);
        assert!(t.cancel(3).is_err(), "已完成不能取消");
        assert!(t.reset_for_retry().is_err(), "已完成不能重试");
    }

    #[test]
    fn failure_and_retry() {
        let mut t = task();
        t.start(1).unwrap();
        t.fail("网络错误".into(), 2).unwrap();
        assert_eq!(t.status, TaskStatus::Failed);
        assert!(t.retry_download().is_err());
        t.reset_for_retry().unwrap();
        assert_eq!(t.status, TaskStatus::Queued);
        assert!(t.error.is_none());
        t.start(3).unwrap();
        assert_eq!(t.attempts, 2);
    }

    #[test]
    fn download_failure_keeps_urls_for_retry() {
        let mut t = task();
        t.start(1).unwrap();
        t.generated(vec!["https://x/a.png".into()]).unwrap();
        t.download_failed("SSL".into(), 2).unwrap();
        assert_eq!(t.status, TaskStatus::DownloadFailed);
        t.retry_download().unwrap();
        assert_eq!(t.status, TaskStatus::Downloading);
        assert_eq!(t.result_urls.len(), 1);
    }

    #[test]
    fn cancel_discards_late_results() {
        let mut t = task();
        t.start(1).unwrap();
        t.cancel(2).unwrap();
        assert!(t.generated(vec!["u".into()]).is_err(), "取消后到达的结果要丢弃");
        assert!(t.set_progress(50).is_err());
        assert!(t.fail("x".into(), 3).is_err());
        assert_eq!(t.status, TaskStatus::Cancelled);
        let mut q = task();
        q.cancel(1).unwrap();
        assert_eq!(q.status, TaskStatus::Cancelled);
    }

    #[test]
    fn restart_recovery() {
        let mut nano = task();
        nano.start(1).unwrap();
        nano.set_remote_id("r9".into()).unwrap();
        assert_eq!(nano.recover(5), Recovery::Poll("r9".into()));
        assert_eq!(nano.status, TaskStatus::Generating);

        let mut no_id = task();
        no_id.start(1).unwrap();
        assert_eq!(no_id.recover(5), Recovery::None);
        assert_eq!(no_id.status, TaskStatus::Failed);

        let mut gpt = task();
        gpt.model = Model::GptImage;
        gpt.start(1).unwrap();
        assert_eq!(gpt.recover(5), Recovery::None, "同步 GPT 无法续上");
        assert_eq!(gpt.status, TaskStatus::Failed);

        let mut dl = task();
        dl.start(1).unwrap();
        dl.generated(vec!["u".into()]).unwrap();
        assert_eq!(dl.recover(5), Recovery::Download);

        let mut queued = task();
        assert_eq!(queued.recover(5), Recovery::None);
        assert_eq!(queued.status, TaskStatus::Queued);
    }

    #[test]
    fn scheduling_respects_concurrency_and_order() {
        let mut tasks = expand_request(&request(Mode::Text, 0, 4), 1, ids()).unwrap();
        assert_eq!(pick_to_start(&tasks, 0, 2), vec!["id2", "id3"]);
        tasks[0].start(1).unwrap();
        tasks[1].start(1).unwrap();
        assert!(pick_to_start(&tasks, 2, 2).is_empty());
        assert_eq!(pick_to_start(&tasks, 1, 2), vec!["id4"]);
        assert_eq!(pick_to_start(&tasks, 2, 5), vec!["id4", "id5"]);
    }

    #[test]
    fn task_json_roundtrip_and_old_files() {
        let t = task();
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"status\":\"queued\""));
        assert!(json.contains("\"model\":\"nano-banana-2.1\""));
        let back: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        let without_flag = json.replace(",\"inQueue\":true", "");
        assert!(serde_json::from_str::<Task>(&without_flag).unwrap().in_queue);
    }
}
