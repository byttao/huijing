//! 绘境 · 后端入口：设置读写、任务队列命令、打开文件夹。

mod api;
mod download;
mod grid;
mod images;
mod queue;
mod settings;
mod split;

use queue::{EnqueueRequest, Queue, Task};
use serde::Serialize;
use settings::Settings;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

struct AppState {
    settings: Arc<RwLock<Settings>>,
    settings_path: PathBuf,
    data_dir: PathBuf,
    default_output_dir: PathBuf,
    queue: Arc<Queue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    settings: Settings,
    /// 实际使用的输出目录
    output_dir: String,
    default_output_dir: String,
    data_dir: String,
    version: String,
    portable: bool,
}

fn view(state: &AppState) -> SettingsView {
    let settings = state.settings.read().unwrap_or_else(|e| e.into_inner()).clone();
    SettingsView {
        output_dir: state.queue.output_dir(&settings).to_string_lossy().to_string(),
        default_output_dir: state.default_output_dir.to_string_lossy().to_string(),
        data_dir: state.data_dir.to_string_lossy().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        portable: cfg!(windows) && state.data_dir.ends_with(settings::PORTABLE_DIR),
        settings,
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> SettingsView {
    view(&state)
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<SettingsView, String> {
    let settings = settings.normalized();
    settings.save(&state.settings_path)?;
    *state.settings.write().unwrap_or_else(|e| e.into_inner()) = settings;
    // 并发数可能变了，按新上限调度
    state.queue.pump();
    Ok(view(&state))
}

/// 测试连接：用表单里尚未保存的设置，不扣费。
#[tauri::command]
async fn test_connection(settings: Settings) -> Result<String, String> {
    let settings = settings.normalized();
    if settings.api_key.is_empty() {
        return Err("请先填写 API Key".into());
    }
    queue::test_api(&settings).await
}

/// 宫格方案预览：与加入队列时用同一套计算（见 grid.rs）。
#[tauri::command]
fn plan_grid(state: State<'_, AppState>, model: api::Model, params: api::GenParams, count: u8) -> Result<grid::GridPlan, String> {
    let upscale = state.settings.read().unwrap_or_else(|e| e.into_inner()).upscale_parts;
    grid::plan(model, &params, count, upscale)
}

#[tauri::command]
fn list_tasks(state: State<'_, AppState>) -> Vec<Task> {
    state.queue.list()
}

#[tauri::command]
fn enqueue(state: State<'_, AppState>, request: EnqueueRequest) -> Result<Vec<Task>, String> {
    state.queue.enqueue(request)
}

#[tauri::command]
fn cancel_task(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.queue.cancel(&id)
}

#[tauri::command]
fn retry_task(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.queue.retry(&id)
}

#[tauri::command]
fn retry_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.queue.retry_download(&id)
}

#[tauri::command]
fn delete_tasks(state: State<'_, AppState>, ids: Vec<String>) {
    state.queue.delete(&ids)
}

#[tauri::command]
fn clear_finished(state: State<'_, AppState>) -> usize {
    state.queue.clear_finished()
}

/// 在访达 / 资源管理器中打开文件夹（不存在时先创建）。
#[tauri::command]
fn open_folder(app: AppHandle, path: String) -> Result<(), String> {
    std::fs::create_dir_all(&path).map_err(|e| format!("无法创建文件夹：{e}"))?;
    app.opener().open_path(path, None::<&str>).map_err(|e| format!("无法打开文件夹：{e}"))
}

/// 在访达 / 资源管理器中显示文件并选中。
#[tauri::command]
fn reveal_file(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("文件已不存在（可能被移动或删除）".into());
    }
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| format!("无法打开所在文件夹：{e}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = settings::resolve_data_dir(app.path().app_config_dir().ok());
            let settings_path = data_dir.join("settings.json");
            let settings = Arc::new(RwLock::new(Settings::load(&settings_path)));
            let default_output_dir = app
                .path()
                .picture_dir()
                .or_else(|_| app.path().home_dir().map(|h| h.join("Pictures")))
                .unwrap_or_else(|_| data_dir.clone())
                .join("绘境");
            let queue = Queue::new(app.handle().clone(), settings.clone(), data_dir.join("tasks.json"), default_output_dir.clone());
            queue.start();
            app.manage(AppState { settings, settings_path, data_dir, default_output_dir, queue });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            test_connection,
            plan_grid,
            list_tasks,
            enqueue,
            cancel_task,
            retry_task,
            retry_download,
            delete_tasks,
            clear_finished,
            open_folder,
            reveal_file,
        ])
        .run(tauri::generate_context!())
        .expect("启动绘境失败");
}

#[cfg(test)]
mod tests {
    /// 发布前的版本一致性：Cargo.toml、tauri.conf.json、package.json 必须同一版本
    /// （CI 另外校验 v* 标签与 tauri.conf.json 一致）
    #[test]
    fn versions_are_consistent() {
        let read = |p: &str| -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(p)).unwrap()).unwrap()
        };
        let cargo = env!("CARGO_PKG_VERSION");
        assert_eq!(read("tauri.conf.json")["version"], cargo, "tauri.conf.json 与 Cargo.toml 版本不一致");
        assert_eq!(read("../package.json")["version"], cargo, "package.json 与 Cargo.toml 版本不一致");
    }
}
