//! 应用专属按键配置（一个应用一个文件）的状态查询、编辑、重载与「点图标」动作。

use tauri::State;

use crate::{load_app_profiles, AppState};

/// 当前前台应用与命中的应用配置。
///
/// 诊断页用它核对「进程名是否写对」——这是新增一份应用配置的第一步。
#[tauri::command]
pub fn app_profile_status(state: State<AppState>) -> core_dispatch::ForegroundStatus {
    state.dispatcher.foreground_status()
}

/// 快捷菜单内圈要显示的应用图标（只含配了 `icon` 的配置）。
///
/// 声明成 `async` 是纵深防御：它会遍历本机所有顶层窗口，虽然实测每次只要
/// 约 2 毫秒，但没有理由让 UI 线程替它站岗——真撞上某个窗口出问题时，至少
/// 界面还能动，日志里的「开始收集 / 完成」也能指出卡在哪。
///
/// 返回 `Result` 只是 Tauri 对「带引用的 async 命令」的硬性要求，这里不会失败；
/// 成功时前端拿到的仍是数组本身。
#[tauri::command]
pub async fn app_menu_apps(
    state: State<'_, AppState>,
) -> Result<Vec<core_dispatch::MenuAppEntry>, String> {
    core_log::log_line("[app-profile] 开始收集菜单应用");
    let apps = state.dispatcher.menu_apps();
    core_log::log_line(&format!(
        "[app-profile] 菜单应用收集完成：{} 个",
        apps.len()
    ));
    Ok(apps)
}

/// 「点图标」：已打开就切到前台，没打开就按配置启动。
///
/// **不需要手动切换按键配置**：映射是按前台应用实时解析的，目标应用一旦到了
/// 前台，下一按键就自动走它的专属映射。
///
/// # 为什么是 async，且把系统调用丢进阻塞线程池
///
/// Tauri 的**同步**命令跑在主线程上。`ShellExecuteW` 走 MSIX 的进程外激活时
/// 需要调用方泵消息，主线程被命令占住就会死锁——实测点 Claude 时整个应用被
/// Windows 判定为「无响应」（AppHangB1）后强杀。`SetForegroundWindow` 同样可能
/// 卡在目标线程上。所以这里：命令声明成 `async`（不再占主线程），两个可能长时间
/// 阻塞的系统调用再各丢进 `spawn_blocking`。
///
/// # 顺序
///
/// - **聚焦**必须趁快捷菜单还是前台窗口时做：`SetForegroundWindow` 只对已拥有
///   前台权限的进程放行，菜单一收起权限就没了，所以先聚焦、后收菜单。
/// - **启动**不需要前台权限（新进程本来就会拿到前台），而且可能要等很久，所以
///   先收菜单让界面立刻有反馈，再去启动。
#[tauri::command]
pub async fn open_app_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    core_log::log_line(&format!("[app-profile] 收到打开请求：{id}"));

    // 定位用 id（文件名），**不用展示名**——展示名在编辑器里可以改，
    // 拿它当身份的话，用户改个名字快捷菜单就点空了。
    let Some(profile) = state.dispatcher.profile_by_id(&id) else {
        return Err(format!("找不到 id 为「{id}」的应用配置"));
    };
    // 下面所有给人看的提示都用展示名。
    let name = profile.display_name();

    // 找窗口很快（一次 EnumWindows），留在当前线程即可。
    if let Some(window) = core_app_profile::find_window(&profile) {
        let target = window.clone();
        let focused = match run_blocking(move || core_app_profile::focus(&target)).await {
            Ok(inner) => inner,
            Err(e) => {
                hide_quick_menu(&app, &state);
                return Err(format!("{name}：{e}"));
            }
        };

        // 先收菜单：无论成败都要让位——成功了要把前台交给目标应用，失败了
        // 也得让用户能继续操作。
        hide_quick_menu(&app, &state);

        return match focused {
            Ok(()) => {
                core_log::log_info(&format!(
                    "[app-profile] 已聚焦 {name}（{} | {}）",
                    window.process, window.title
                ));
                Ok(format!("已切到 {name}"))
            }
            Err(e) => {
                core_log::log_warn(&format!("[app-profile] {name} 聚焦失败：{e}"));
                Err(format!("{name}：{e}"))
            }
        };
    }

    let Some(spec) = profile.launch.clone() else {
        hide_quick_menu(&app, &state);
        core_log::log_warn(&format!("[app-profile] {name} 既没运行也没配启动方式"));
        return Err(format!("没找到 {name} 的窗口，这份配置也没写启动方式"));
    };

    hide_quick_menu(&app, &state);

    match run_blocking(move || core_app_profile::launch(&spec)).await {
        Ok(Ok(())) => {
            core_log::log_info(&format!("[app-profile] 已启动 {name}"));
            Ok(format!("正在启动 {name}"))
        }
        Ok(Err(e)) => {
            core_log::log_warn(&format!("[app-profile] {name} 启动失败：{e}"));
            Err(format!("{name}：{e}"))
        }
        Err(e) => Err(format!("{name}：{e}")),
    }
}

/// 把可能长时间阻塞的系统调用丢进阻塞线程池，别占着 Tauri 的线程。
async fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("后台任务失败：{e}"))
}

/// 收起快捷菜单；失败只记日志，不改变「点图标」本身的结果。
fn hide_quick_menu(app: &tauri::AppHandle, state: &State<'_, AppState>) {
    if let Err(e) = crate::commands::quick_menu::hide_quick_menu(app, state) {
        core_log::log_warn(&format!("[app-profile] 收起快捷菜单失败：{e}"));
    }
}

/// 重新加载应用专属配置（先播种，再读 `app-profiles/`），返回加载后的配置总数。
///
/// 用户直接改了 `<配置目录>/app-profiles/` 里的文件后，不必重启应用。
#[tauri::command]
pub fn reload_app_profiles(state: State<AppState>) -> usize {
    let registry = load_app_profiles();
    let count = registry.profiles().len();
    state.dispatcher.set_profiles(registry);
    core_log::log_info(&format!("[commands/app_profile] 已重载 {count} 份应用配置"));
    count
}

// ---------------------------------------------------------------------------
// 编辑器用的读写命令
//
// 每个写命令都是「读出来 → 改一处 → 整份写回 → 重载调度器」。重载**放在命令
// 内部**而不是让前端再调一次 `reload_app_profiles`：顺序一旦交给前端，就有
// 「文件写了但调度器没更新」的窗口，表现成「保存了但按键没反应」。
// ---------------------------------------------------------------------------

/// 一份应用配置的完整内容，供「按键映射」页的作用范围选择器与矩阵使用。
#[derive(serde::Serialize)]
pub struct AppProfileView {
    /// 稳定标识（`app-profiles/` 里的文件名）。保存 / 删除都靠它定位。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// 展示用的进程名写法；纯标题配置为空串。
    pub process: String,
    /// 窗口标题关键字。
    pub title_contains: Vec<String>,
    /// 快捷菜单内圈用的品牌色（形如 `#F59E0B`）；没配图标时为空。
    ///
    /// 作用范围胶囊上的小圆点用它，和快捷菜单内圈保持一致。
    pub icon_color: Option<String>,
    pub note: String,
    /// 这份配置覆盖了哪些 (按键, 触发)。没列出的格子沿用全局。
    pub bindings: Vec<crate::MappingEntry>,
}

impl AppProfileView {
    fn from_profile(p: &core_app_profile::AppProfile) -> Self {
        Self {
            id: p.id.clone(),
            name: p.display_name(),
            process: p.process.display(),
            title_contains: p.window_title_contains.clone(),
            icon_color: p.icon.as_ref().map(|icon| icon.color.clone()),
            note: p.note.clone(),
            bindings: p.bindings.iter().map(binding_entry).collect(),
        }
    }
}

fn binding_entry(b: &core_mapping::KeyBinding) -> crate::MappingEntry {
    crate::MappingEntry {
        button: crate::button_key(&b.button),
        name: b.button.display_name().to_string(),
        trigger: crate::trigger_key(&b.trigger),
        action: crate::action_label(&b.action),
        action_key: crate::action_key(&b.action),
    }
}

/// 全部应用配置。**含没有图标的那些**——图标只决定要不要进快捷菜单内圈，
/// 不影响它有没有资格出现在作用范围里。
#[tauri::command]
pub fn app_profile_catalog(state: State<AppState>) -> Vec<AppProfileView> {
    state
        .dispatcher
        .profiles_snapshot()
        .iter()
        .map(AppProfileView::from_profile)
        .collect()
}

/// 新建一份应用配置，返回新配置的 id（前端随后切到这个作用范围）。
///
/// 进程名与窗口标题关键字**至少要填一个**：两个都空的配置永远不会命中，
/// 建出来就是个死配置，不如直接拒绝。
#[tauri::command]
pub fn create_app_profile(
    name: String,
    process: String,
    window_title_contains: String,
    state: State<AppState>,
) -> Result<String, String> {
    let dir = config_dir()?;
    let processes = split_list(&process);
    let titles = split_list(&window_title_contains);
    if processes.is_empty() && titles.is_empty() {
        return Err("进程名和窗口标题关键字至少要填一个，否则这份配置永远不会生效".into());
    }

    let id = core_app_profile::new_profile_id(&dir, &process);
    let profile = core_app_profile::AppProfile {
        id: id.clone(),
        process: match processes.len() {
            0 => core_app_profile::ProcessSpec::Many(Vec::new()),
            1 => core_app_profile::ProcessSpec::One(processes[0].clone()),
            _ => core_app_profile::ProcessSpec::Many(processes),
        },
        name: name.trim().to_string(),
        window_title_contains: titles,
        ..Default::default()
    };

    core_app_profile::save_profile(&dir, &profile).map_err(|e| format!("写入配置失败：{e}"))?;
    reload_profiles(&state);
    core_log::log_info(&format!(
        "[app-profile] 新建应用配置 {id}（{}）",
        profile.display_name()
    ));
    Ok(id)
}

/// 把某个 (按键, 触发) 写进这份配置（已存在则改掉）。
#[tauri::command]
pub fn save_profile_binding(
    id: String,
    button: String,
    trigger: String,
    action: String,
    state: State<AppState>,
) -> Result<(), String> {
    let button = crate::parse_button(&button).ok_or("未知按键")?;
    let trigger = crate::parse_trigger(&trigger).ok_or("未知触发")?;
    let action = crate::parse_action(&action).ok_or("未知动作")?;

    let dir = config_dir()?;
    let mut profile = find_profile(&state, &id)?;
    match profile
        .bindings
        .iter_mut()
        .find(|b| b.button == button && b.trigger == trigger)
    {
        Some(binding) => binding.action = action,
        None => profile.bindings.push(core_mapping::KeyBinding {
            button,
            trigger,
            action,
        }),
    }
    core_app_profile::save_profile(&dir, &profile).map_err(|e| format!("写入配置失败：{e}"))?;
    reload_profiles(&state);
    Ok(())
}

/// 「改用全局」：把这一个 (按键, 触发) 从这个应用配置里移掉，其余格子不动。
///
/// 注意它和删除整份配置是两件事——删整份走 [`delete_app_profile`]。
#[tauri::command]
pub fn clear_profile_binding(
    id: String,
    button: String,
    trigger: String,
    state: State<AppState>,
) -> Result<(), String> {
    let button = crate::parse_button(&button).ok_or("未知按键")?;
    let trigger = crate::parse_trigger(&trigger).ok_or("未知触发")?;

    let dir = config_dir()?;
    let mut profile = find_profile(&state, &id)?;
    let before = profile.bindings.len();
    profile
        .bindings
        .retain(|b| !(b.button == button && b.trigger == trigger));
    if profile.bindings.len() == before {
        // 本来就没覆盖过这一格，无事可做——不写文件，也不重载。
        return Ok(());
    }
    core_app_profile::save_profile(&dir, &profile).map_err(|e| format!("写入配置失败：{e}"))?;
    reload_profiles(&state);
    Ok(())
}

/// 删掉整份应用配置，该应用的所有按键回到全局。
#[tauri::command]
pub fn delete_app_profile(id: String, state: State<AppState>) -> Result<(), String> {
    let dir = config_dir()?;
    // 先确认它真的存在，免得把「id 拼错了」当成删除成功。
    let profile = find_profile(&state, &id)?;
    core_app_profile::remove_profile(&dir, &id).map_err(|e| format!("删除配置失败：{e}"))?;
    reload_profiles(&state);
    core_log::log_info(&format!(
        "[app-profile] 已删除应用配置 {id}（{}）",
        profile.display_name()
    ));
    Ok(())
}

/// 配置目录（`%LOCALAPPDATA%\RemoteMic\RC003`）。
fn config_dir() -> Result<std::path::PathBuf, String> {
    crate::config_store()
        .map(|store| store.dir)
        .ok_or_else(|| "无法创建配置目录".to_string())
}

/// 从当前注册表里取一份配置的副本。
fn find_profile(
    state: &State<'_, AppState>,
    id: &str,
) -> Result<core_app_profile::AppProfile, String> {
    state
        .dispatcher
        .profile_by_id(id)
        .ok_or_else(|| format!("找不到 id 为「{id}」的应用配置"))
}

/// 改完文件后重新加载，让调度器立刻用上新配置。
fn reload_profiles(state: &State<'_, AppState>) {
    state.dispatcher.set_profiles(load_app_profiles());
}

/// 把「逗号 / 中文逗号 / 换行」分隔的输入拆成条目列表。
fn split_list(text: &str) -> Vec<String> {
    text.split([',', '，', '\n', '\r'])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
