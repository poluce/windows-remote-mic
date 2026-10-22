//! 内存中的应用配置集合与快速匹配。

use std::path::Path;

use super::model::AppProfile;
use super::seeds::seed_templates;
use super::store::{load_dir, parse_profile};

/// 已加载的应用配置集合。
#[derive(Debug, Clone, Default)]
pub struct ProfileRegistry {
    profiles: Vec<AppProfile>,
}

impl ProfileRegistry {
    /// 解析编译进来的种子，**不落盘**。
    ///
    /// 正常的加载路径不经过这里：种子先由 [`seed_user_dir`](crate::seeds::seed_user_dir) 落地成文件，再走
    /// [`load`](Self::load) 读回来。这个入口留给播种逻辑的自检与测试——需要在
    /// 没有配置目录的环境里拿一份真实的配置集合时用它。
    pub fn from_seeds() -> Self {
        let mut registry = Self::default();
        for (id, content) in seed_templates() {
            match parse_profile(&id, content) {
                Ok(profile) => registry.upsert(profile),
                Err(e) => core_log::log_warn(&format!(
                    "[app-profile] 种子 {id}.json 解析失败，已跳过：{e}"
                )),
            }
        }
        registry
    }

    /// 加载用户目录下的全部配置。
    ///
    /// 目录不存在（还没播种过，或用户把配置删光了）就是空注册表，不是错误。
    pub fn load(user_dir: Option<&Path>) -> Self {
        let mut registry = Self::default();
        if let Some(dir) = user_dir {
            for profile in load_dir(dir) {
                registry.upsert(profile);
            }
        }
        registry
    }

    /// 插入配置；与已有配置抢同一批窗口时，后者整份替换前者。
    ///
    /// 判重用 [`AppProfile::conflicts_with`]，进程名和标题关键字都算。这不是
    /// 「用户覆盖内置」那条老路了（内置已经不存在），而是防止两份用户文件互相
    /// 遮蔽：匹配用的是 `.find()`，真撞上时只有先加载的那份生效，另一份会表现成
    /// 「改了没反应」，所以至少要留一条日志。
    pub fn upsert(&mut self, profile: AppProfile) {
        match self
            .profiles
            .iter()
            .position(|p| p.conflicts_with(&profile))
        {
            Some(idx) => {
                if self.profiles[idx].id != profile.id {
                    core_log::log_warn(&format!(
                        "[app-profile] 「{}」与「{}」会匹配同一批窗口，后者生效",
                        self.profiles[idx].display_name(),
                        profile.display_name()
                    ));
                }
                self.profiles[idx] = profile;
            }
            None => self.profiles.push(profile),
        }
    }

    /// 全部配置，顺序即目录里的加载顺序。
    pub fn profiles(&self) -> &[AppProfile] {
        &self.profiles
    }

    /// 按 id（文件名）取配置。
    ///
    /// 保存 / 删除 / 快捷菜单「点图标」都用它定位目标。**不要改用展示名**：
    /// `name` 在界面上可改，改了之后所有按名字找的地方都会落空。
    pub fn get_by_id(&self, id: &str) -> Option<&AppProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// 按前台进程名匹配配置；大小写不敏感。传完整路径也可以，只看文件名。
    pub fn match_process(&self, exe_name: &str) -> Option<&AppProfile> {
        let name = exe_name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(exe_name)
            .trim();
        self.profiles.iter().find(|p| p.process.matches(name))
    }

    /// 按「进程名 + 窗口标题」匹配配置，进程名优先。
    ///
    /// 标题兜底是为进程名认不出来的目标准备的：Chrome PWA、以及跑在 WSL 里
    /// 的服务（DSH 只有浏览器窗口是 Windows 进程）。
    pub fn match_context(&self, exe_name: &str, window_title: &str) -> Option<&AppProfile> {
        let name = exe_name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(exe_name)
            .trim();
        self.profiles
            .iter()
            .find(|p| p.process.matches(name))
            .or_else(|| self.profiles.iter().find(|p| p.matches_title(window_title)))
    }

    /// 配了图标的配置，用于快捷菜单内圈；顺序即文件里的声明顺序。
    pub fn menu_entries(&self) -> Vec<&AppProfile> {
        self.profiles
            .iter()
            .filter(|p| p.icon.as_ref().is_some_and(|i| !i.label.trim().is_empty()))
            .collect()
    }
}
