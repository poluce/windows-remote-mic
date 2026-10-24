//! 应用专属按键配置在编辑器中的增删改查。
//!
//! 每个写命令都是「读出来 → 改一处 → 整份写回 → 重载调度器」。重载放在命令内部
//! 而不是让前端再调一次 `reload_app_profiles`，防止出现「保存了但调度器未更新」的时间窗口。

use tauri::State;

use crate::{load_app_profiles, AppState};

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
        enabled: b.enabled,
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
        Some(binding) => {
            binding.action = action;
            binding.enabled = true;
        }
        None => profile.bindings.push(core_mapping::KeyBinding {
            button,
            trigger,
            action,
            enabled: true,
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
