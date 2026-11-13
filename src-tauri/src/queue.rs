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
use crate::grid::{self, GridPlan};
use image::GenericImageView;
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
    /// 多合一张数（1 = 普通单张）
    #[serde(default = "default_one")]
    pub count: u8,
    /// 宫格方案（多合一时用于请求与拆切；单张时记录尺寸）
    #[serde(default)]
    pub grid: Option<GridPlan>,
    /// 实际发给接口的提示词（多合一时外包宫格指令）；`prompt` 始终是用户原文
    #[serde(default)]
    pub sent_prompt: Option<String>,
    /// 多合一的合成原图（拆切后 `files` 是各张）
    #[serde(default)]
    pub composite_file: Option<String>,
    /// 非致命提示，例如拆切失败只保留了合成原图
    #[serde(default)]
    pub notice: Option<String>,
    /// 持久的自增序号（#12），删除后不复用；旧记录按创建时间补号
    #[serde(default)]
    pub number: u64,
    /// 「改自」的原记录序号（原记录删了也能显示「改自 #7」）
    #[serde(default)]
    pub parent_number: Option<u64>,
    /// 改自原记录多合一的第几张（从 1 开始），用于显示「改自 #7-2」
    #[serde(default)]
    pub parent_part: Option<u32>,
}

/// 给没有序号的记录按创建时间补号，并补全「改自」的原记录序号与切片号。
/// `next` 是文件里记下的下一个序号（保证删除后不复用），返回新的 next。
pub fn assign_numbers(tasks: &mut [Task], next: u64) -> u64 {
    let max = tasks.iter().map(|t| t.number).max().unwrap_or(0);
    let mut next = next.max(max + 1);
    let mut missing: Vec<usize> = (0..tasks.len()).filter(|&i| tasks[i].number == 0).collect();
    missing.sort_by_key(|&i| (tasks[i].created_at, i));
    for i in missing {
        tasks[i].number = next;
        next += 1;
    }
    let lookup: HashMap<String, (u64, Vec<String>)> = tasks.iter().map(|t| (t.id.clone(), (t.number, t.files.clone()))).collect();
    for t in tasks.iter_mut() {
        if t.parent_number.is_none()
            && let Some((number, files)) = t.parent_id.as_ref().and_then(|p| lookup.get(p))
        {
            t.parent_number = Some(*number);
            t.parent_part = part_index(files, t.ref_images.first());
        }
    }
    next
}

/// 参考图是原记录多合一的第几张（单张记录返回 None）
fn part_index(files: &[String], reference: Option<&String>) -> Option<u32> {
    if files.len() < 2 {
        return None;
    }
    let r = reference?;
    files.iter().position(|f| f == r).map(|i| i as u32 + 1)
}

fn default_one() -> u8 {
    1
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
        self.composite_file = None;
        self.notice = None;
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

/// 校验请求并生成一个任务。`count` > 1 时是「多合一」：仍然只有一个任务、一次请求，
/// 结果按宫格拆成 count 张。`upscale` 来自设置「拆切后放大到单张原尺寸」。
pub fn build_task(req: &EnqueueRequest, now: i64, upscale: bool, mut next_id: impl FnMut() -> String) -> Result<Task, String> {
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
    let count = req.count.clamp(1, 4);
    let plan = grid::plan(req.model, &req.params, count, upscale)?;
    let sent_prompt = grid::wrap_prompt(prompt, &plan);
    let id = next_id();
    Ok(Task {
        batch_id: id.clone(),
        id,
        seq: 1,
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
        count,
        sent_prompt: (sent_prompt != prompt).then_some(sent_prompt),
        grid: Some(plan),
        composite_file: None,
        notice: None,
        number: 0,
        parent_number: None,
        parent_part: None,
    })
}

/// 实际请求用的提示词与参数：多合一时换成宫格提示词、合成比例与（可能升档的）分辨率。
pub fn request_of(task: &Task) -> (String, GenParams) {
    let prompt = task.sent_prompt.clone().unwrap_or_else(|| task.prompt.clone());
    let mut params = task.params.clone();
    if let Some(plan) = task.grid.as_ref().filter(|p| p.count > 1) {
        params.aspect_ratio = plan.request_ratio.clone();
        if !plan.request_tier.is_empty() {
            params.image_size = plan.request_tier.clone();
        }
    }
    (prompt, params)
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
    /// 下一个任务序号；删除记录后也不回退
    #[serde(default, rename = "nextNumber")]
    next_number: u64,
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
    next_number: u64,
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
        let (mut tasks, next) = std::fs::read_to_string(&tasks_path)
            .ok()
            .and_then(|text| serde_json::from_str::<TaskFile>(&text).ok())
            .map(|f| (f.tasks, f.next_number))
            .unwrap_or_default();
        let next_number = assign_numbers(&mut tasks, next);
        Arc::new(Self {
            app,
            settings,
            tasks_path,
            default_output_dir,
            state: Mutex::new(State { tasks, running: HashMap::new(), next_number }),
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
        let file = TaskFile { version: 1, tasks: st.tasks.clone(), next_number: st.next_number };
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
        let mut task = build_task(&req, now_ms(), settings.upscale_parts, || self.next_id())?;
        let mut st = self.lock();
        task.number = st.next_number;
        st.next_number += 1;
        if let Some(parent) = task.parent_id.as_ref().and_then(|p| st.tasks.iter().find(|t| &t.id == p)) {
            task.parent_number = Some(parent.number);
            task.parent_part = part_index(&parent.files, task.ref_images.first());
        }
        let tasks = vec![task.clone()];
        st.tasks.push(task.clone());
        self.emit(&task);
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
        let (prompt, params) = request_of(task);
        let req = GenRequest { model: task.model, prompt, params, images };
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
            let mut name = download::build_file_name(task.created_at, task.model.id(), task.seq, i, &download::ext_from_url(url, fallback));
            if task.count > 1
                && let Some((stem, ext)) = name.rsplit_once('.')
            {
                name = format!("{stem}-宫格.{ext}");
            }
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
        // 多合一：按宫格拆切（失败时保留合成原图，任务仍算完成）
        let plan = task.grid.clone().filter(|p| p.count > 1);
        let (files, composite, notice) = match plan {
            None => (files, None, None),
            Some(plan) => {
                self.log("processing", format!("{} 下载完成，按 {}×{} 宫格拆切…", task.label(), plan.cols, plan.rows));
                let composites = files.clone();
                let out_dir = dir.clone();
                let split = tauri::async_runtime::spawn_blocking(move || split_composites(&composites, &plan, &out_dir))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r);
                match split {
                    Ok(parts) => (parts, files.first().cloned(), None),
                    Err(e) => (files.clone(), files.first().cloned(), Some(format!("拆切失败：{e}，已保留合成原图"))),
                }
            }
        };
        let count = files.len();
        let note = notice.clone();
        if self
            .update(id, true, |t| {
                t.downloaded(files, now_ms())?;
                t.composite_file = composite;
                t.notice = notice;
                Ok(())
            })
            .is_some()
        {
            match note {
                Some(n) => self.log("warning", format!("{} 已下载，但{n}", task.label())),
                None => self.log("success", format!("{} 已完成，保存 {count} 张到 {}", task.label(), dir.display())),
            }
        }
    }

    fn finish_download_failed(&self, task: &Task, message: String) {
        // 下载失败可单独「重试下载」，不重新扣费
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

/// 把合成原图按宫格等分拆成多张 PNG，写在原图旁边：`…_01-宫格.png` → `…_01-1.png` … 。
/// 开了「拆切后放大」时每张用 Lanczos3 等比放大到整张图的长边（以实际合成图为准）。
pub fn split_composites(composites: &[String], plan: &GridPlan, dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for composite in composites {
        let path = Path::new(composite);
        let img = image::open(path).map_err(|e| format!("读取合成图失败：{e}"))?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let stem = stem.strip_suffix("-宫格").unwrap_or(&stem).to_string();
        let target_long = grid::target_long_side(img.dimensions(), plan);
        for (k, part) in crate::split::split_grid(&img, plan.cols as u32, plan.rows as u32).into_iter().enumerate() {
            let size = [part.width(), part.height()];
            let part = match target_long {
                Some(long) if long > size[0].max(size[1]) => {
                    let [w, h] = grid::scale_to_long(size, long);
                    crate::split::resize(&part, w, h)?
                }
                _ => part,
            };
            let dest = download::reserve_unique(dir, &format!("{stem}-{}.png", k + 1)).map_err(|e| format!("创建文件失败：{e}"))?;
            part.save_with_format(&dest, image::ImageFormat::Png).map_err(|e| format!("保存拆切图失败：{e}"))?;
            out.push(dest.to_string_lossy().to_string());
        }
    }
    Ok(out)
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
        build_task(&request(Mode::Text, 0, 1), 1000, true, ids()).unwrap()
    }

    #[test]
    fn build_task_validation_and_grid() {
        let t = build_task(&request(Mode::Text, 0, 1), 1, true, ids()).unwrap();
        assert_eq!((t.count, t.prompt.as_str(), t.status), (1, "水墨山水", TaskStatus::Queued));
        assert!(t.sent_prompt.is_none(), "单张不包装提示词");
        assert!(build_task(&request(Mode::Image, 0, 1), 1, true, ids()).is_err());
        assert!(build_task(&request(Mode::Text, 1, 1), 1, true, ids()).is_err());
        let mut empty = request(Mode::Text, 0, 1);
        empty.prompt = "   ".into();
        assert!(build_task(&empty, 1, true, ids()).is_err());
        let mut edit = request(Mode::Edit, 1, 1);
        edit.parent_id = Some("p1".into());
        assert_eq!(build_task(&edit, 1, true, ids()).unwrap().parent_id.as_deref(), Some("p1"));

        // 多合一：仍是一个任务，提示词被包装，请求参数换成合成比例
        let mut multi = request(Mode::Text, 0, 4);
        multi.params.aspect_ratio = "16:9".into();
        multi.params.higher_res = true;
        let t = build_task(&multi, 1, true, ids()).unwrap();
        assert_eq!(t.count, 4);
        assert_eq!(t.prompt, "水墨山水");
        assert!(t.sent_prompt.as_deref().unwrap().contains("2 列 × 2 行"));
        let (prompt, params) = request_of(&t);
        assert!(prompt.ends_with("水墨山水"));
        assert_eq!((params.aspect_ratio.as_str(), params.image_size.as_str()), ("16:9", "2K"));
        assert_eq!(t.params.image_size, "1K", "任务里保存的仍是用户参数");

        // 比例 auto 也允许多合一（每张比例取决于实际出图）
        let mut auto = request(Mode::Text, 0, 2);
        auto.params.aspect_ratio = "auto".into();
        let t = build_task(&auto, 1, true, ids()).unwrap();
        assert_eq!(request_of(&t).1.aspect_ratio, "auto");
    }

    #[test]
    fn split_composite_files() {
        use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
        let dir = std::env::temp_dir().join(format!("huijing-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut img = RgbImage::new(400, 400);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([(x / 2) as u8, (y / 2) as u8, ((x + y) % 200) as u8]);
        }
        let composite = dir.join("a_01-宫格.png");
        DynamicImage::ImageRgb8(img).save(&composite).unwrap();
        let mut req = request(Mode::Text, 0, 4);
        req.model = Model::GptImage;
        req.params.aspect_ratio = "1:1".into();
        let plan = build_task(&req, 1, true, ids()).unwrap().grid.unwrap();
        let parts = split_composites(&[composite.to_string_lossy().to_string()], &plan, &dir).unwrap();
        assert_eq!(parts.len(), 4);
        assert!(parts[0].ends_with("a_01-1.png") && parts[3].ends_with("a_01-4.png"));
        for p in &parts {
            // 2×2 等分后每张 200×200，放大到整张图长边 400
            assert_eq!(image::open(p).unwrap().dimensions(), (400, 400), "放大到整张图的长边");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn numbering_never_reuses_and_backfills_old_records() {
        let mut tasks: Vec<Task> = (0..3)
            .map(|i| {
                let mut t = task();
                t.id = format!("t{i}");
                t.created_at = 100 - i as i64; // 倒序创建：t2 最早
                t
            })
            .collect();
        let next = assign_numbers(&mut tasks, 0);
        assert_eq!((tasks[2].number, tasks[1].number, tasks[0].number, next), (1, 2, 3, 4));
        // 文件里记的 next 比现有最大号大（删过记录），不回退
        let mut t = vec![task()];
        t[0].number = 2;
        assert_eq!(assign_numbers(&mut t, 9), 9);
        // 补「改自」：原记录多合一的第 2 张
        let mut parent = task();
        parent.id = "p".into();
        parent.number = 7;
        parent.files = vec!["/a-1.png".into(), "/a-2.png".into()];
        let mut child = task();
        child.id = "c".into();
        child.parent_id = Some("p".into());
        child.ref_images = vec!["/a-2.png".into()];
        let mut both = vec![parent, child];
        assign_numbers(&mut both, 0);
        assert_eq!((both[1].parent_number, both[1].parent_part), (Some(7), Some(2)));
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
        let mut n = 0;
        let mut tasks: Vec<Task> = (0..4)
            .map(|_| {
                n += 1;
                let mut t = task();
                t.id = format!("t{n}");
                t
            })
            .collect();
        assert_eq!(pick_to_start(&tasks, 0, 2), vec!["t1", "t2"]);
        tasks[0].start(1).unwrap();
        tasks[1].start(1).unwrap();
        assert!(pick_to_start(&tasks, 2, 2).is_empty());
        assert_eq!(pick_to_start(&tasks, 1, 2), vec!["t3"]);
        assert_eq!(pick_to_start(&tasks, 2, 5), vec!["t3", "t4"]);
    }

    #[test]
    fn task_json_roundtrip_and_old_files() {
        let t = task();
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"status\":\"queued\""));
        assert!(json.contains("\"model\":\"nano-banana-2.1\""));
        let back: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        // 旧版本写下的记录没有新字段，也要能读
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        for key in ["inQueue", "count", "grid", "sentPrompt", "compositeFile", "notice", "number", "parentNumber", "parentPart"] {
            v.as_object_mut().unwrap().remove(key);
        }
        let old: Task = serde_json::from_value(v).unwrap();
        assert!(old.in_queue);
        assert_eq!(old.count, 1);
    }

    /// 用真实合成图离线验证拆切：
    /// COMPOSITE=… MODEL=nano-banana-2.1 RATIO=16:9 TIER=1K COUNT=4 OUT=/tmp/x cargo test resplit_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn resplit_real() {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let model: Model = serde_json::from_str(&format!("\"{}\"", env("MODEL"))).unwrap();
        let params = GenParams { aspect_ratio: env("RATIO"), image_size: env("TIER"), ..GenParams::default() };
        let plan = grid::plan(model, &params, env("COUNT").parse().unwrap(), true).unwrap();
        let out = std::path::PathBuf::from(env("OUT"));
        std::fs::create_dir_all(&out).unwrap();
        let src = out.join("composite-宫格.png");
        std::fs::copy(env("COMPOSITE"), &src).unwrap();
        for p in split_composites(&[src.to_string_lossy().to_string()], &plan, &out).unwrap() {
            println!("{p} {:?}", image::open(&p).unwrap().dimensions());
        }
    }
}