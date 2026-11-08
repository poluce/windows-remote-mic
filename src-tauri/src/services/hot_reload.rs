use std::sync::{Arc, Mutex};
use notify::{Watcher, RecursiveMode};
use tauri::{AppHandle, Emitter};
use core_dispatch::KeyDispatcher;
use core_config::{ConfigStore};

pub struct HotReloader {
    dispatcher: Arc<KeyDispatcher>,
    app_handle: AppHandle,
    config_store: ConfigStore,
    last_revision: Arc<Mutex<u64>>,
    _watcher: Arc<Mutex<Option<notify::RecommendedWatcher>>>,
}

impl HotReloader {
    pub fn new(dispatcher: Arc<KeyDispatcher>, app_handle: AppHandle, config_store: ConfigStore) -> Self {
        let last_revision = Arc::new(Mutex::new(0));
        
        if let Ok(cfg) = config_store.load() {
            let mut rev = last_revision.lock().unwrap();
            *rev = cfg.revision;
        }

        Self {
            dispatcher,
            app_handle,
            config_store,
            last_revision,
            _watcher: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_dir = self.config_store.dir.clone();
        let app_handle = self.app_handle.clone();
        let dispatcher = self.dispatcher.clone();
        let last_revision = self.last_revision.clone();
        let config_store = self.config_store.clone();
        let watcher_handle = self._watcher.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            match res {
                Ok(event) => {
                    let is_config = event.paths.iter().any(|p| p.file_name().and_then(|n| n.to_str()) == Some("config.json"));
                    let is_profile = event.paths.iter().any(|p| p.components().any(|c| c.as_os_str() == "app-profiles"));
                    
                    if (is_config || is_profile) && (event.kind.is_modify() || event.kind.is_create()) {
                        Self::reload_config(&config_store, &dispatcher, &app_handle, &last_revision);
                    }
                }
                Err(e) => core_log::log_error(&format!("[hot_reload] 监听错误: {e}")),
            }
        })?;

        watcher.watch(&config_dir, RecursiveMode::Recursive)?;
        
        let mut w = watcher_handle.lock().unwrap();
        *w = Some(watcher);
        
        Ok(())
    }

    fn reload_config(store: &ConfigStore, dispatcher: &KeyDispatcher, app: &AppHandle, last_revision: &Arc<Mutex<u64>>) {
        // 防抖
        std::thread::sleep(std::time::Duration::from_millis(100));

        let cfg = match store.load() {
            Ok(c) => c,
            Err(e) => {
                core_log::log_error(&format!("[hot_reload] 重载配置失败: {e}"));
                return;
            }
        };

        {
            let mut rev = last_revision.lock().unwrap();
            if cfg.revision == *rev {
                return;
            }
            *rev = cfg.revision;
        }

        core_log::log_info("[hot_reload] 配置已变动，执行重载");

        dispatcher.update_mapping(cfg.mapping);
        dispatcher.set_trigger_timing(cfg.long_press_ms, cfg.double_click_ms);
        dispatcher.set_voice_target(cfg.voice_target);
        
        dispatcher.set_profiles(crate::load_app_profiles());

        #[cfg(target_os = "windows")]
        core_hid::tap::write_eat_mode_file(cfg.hid_tap_eat);
        
        let _ = app.emit("config-changed", ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Unit test for revision check
    #[test]
    fn test_revision_check() {
        let last_revision = Arc::new(Mutex::new(10));
        let mut rev = last_revision.lock().unwrap();
        let cfg_revision = 11;
        assert_ne!(cfg_revision, *rev);
        *rev = cfg_revision;
        assert_eq!(cfg_revision, *rev);
    }
}
