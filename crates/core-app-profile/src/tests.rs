//! core-app-profile 单元测试。

use std::path::Path;

use super::seeds::*;
use super::store::*;
use super::*;
use core_mapping::{ActionKind, ButtonId, KeyBinding, Trigger};

fn profile(process: &str, name: &str) -> AppProfile {
    AppProfile {
        process: ProcessSpec::One(process.into()),
        name: name.into(),
        bindings: vec![KeyBinding {
            button: ButtonId::Ok,
            trigger: Trigger::SingleClick,
            action: ActionKind::Return,
        }],
        ..Default::default()
    }
}

#[test]
fn seed_profiles_are_embedded_and_parse() {
    let registry = ProfileRegistry::from_seeds();
    // build.rs 会把仓库根 profiles/*.json 编进来；仓库里至少要有 Codex 这份。
    assert!(
        !registry.profiles().is_empty(),
        "没有编译进任何应用配置种子，检查 profiles/ 与 build.rs"
    );
    for p in registry.profiles() {
        assert!(
            !p.process.is_empty() || !p.window_title_contains.is_empty(),
            "种子既没有 process 也没有 window_title_contains，永远匹配不上：{}",
            p.display_name()
        );
        assert!(
            !p.display_name().is_empty(),
            "种子缺少展示名：{:?}",
            p.process
        );
        assert!(!p.id.is_empty(), "种子没有 id：{}", p.display_name());
    }
}

#[test]
fn match_process_is_case_insensitive_and_accepts_paths() {
    let registry = ProfileRegistry::from_seeds();
    // 用种子里第一个**带进程名**的配置反查，避免绑死在某个具体应用上，
    // 也避免碰上纯标题配置（它的 process 是空的）。
    let Some(first) = registry.profiles().iter().find(|p| !p.process.is_empty()) else {
        return;
    };
    let exe = first.process.names()[0].clone();

    assert!(registry.match_process(&exe).is_some());
    assert!(registry.match_process(&exe.to_uppercase()).is_some());
    assert!(registry
        .match_process(&format!(r"C:\Apps\Whatever\{exe}"))
        .is_some());
    assert!(registry
        .match_process("definitely-not-an-app.exe")
        .is_none());
}

#[test]
fn upsert_replaces_profile_matching_the_same_process() {
    let mut registry = ProfileRegistry::default();
    registry.upsert(profile("MyApp.exe", "第一版"));
    registry.upsert(profile("MyApp.exe", "第二版"));

    assert_eq!(registry.profiles().len(), 1, "同进程名应整份替换而不是新增");
    assert_eq!(
        registry.match_process("MyApp.exe").unwrap().display_name(),
        "第二版"
    );
}

#[test]
fn upsert_replaces_title_only_profile_matching_the_same_title() {
    // 修复前这里会留下两份：判重只看进程名，而纯标题配置的 process 是空的，
    // 谁都撞不上。匹配用的是 .find()，于是后加的那份永远不生效——
    // 表现出来就是「界面里改了，实际没反应」。
    let mut first = profile("", "DeepSeek Harness");
    first.window_title_contains = vec!["DeepSeek Harness".into()];
    let mut second = profile("", "DSH 改过的");
    second.window_title_contains = vec!["deepseek harness".into()]; // 大小写不同也算同一批窗口

    let mut registry = ProfileRegistry::default();
    registry.upsert(first);
    registry.upsert(second);

    assert_eq!(registry.profiles().len(), 1, "同标题应整份替换");
    assert_eq!(
        registry
            .match_context("chrome.exe", "打招呼 · DeepSeek Harness")
            .unwrap()
            .display_name(),
        "DSH 改过的"
    );
}

#[test]
fn id_tracks_the_file_name_and_stays_out_of_json() {
    let dir = tempfile::tempdir().unwrap();
    let user_dir = dir.path().join(USER_PROFILE_DIR);
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::write(
        user_dir.join("my-app.json"),
        r#"{ "process": "MyApp.exe", "name": "我的应用" }"#,
    )
    .unwrap();

    let registry = ProfileRegistry::load(Some(&user_dir));
    let hit = registry.get_by_id("my-app").expect("id 应取自文件名");
    assert_eq!(hit.display_name(), "我的应用");
    assert!(registry.get_by_id("我的应用").is_none(), "展示名不是身份");

    // 身份不能跟着配置写进文件——它是文件名给的。
    let text = serde_json::to_string(hit).unwrap();
    assert!(!text.contains("\"id\""), "id 不该被序列化：{text}");
}

#[test]
fn user_dir_loads_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let user_dir = dir.path().join(USER_PROFILE_DIR);
    std::fs::create_dir_all(&user_dir).unwrap();

    std::fs::write(
            user_dir.join("my-app.json"),
            r#"{ "process": "MyApp.exe", "name": "我的应用",
                 "bindings": [ { "button": "Ok", "trigger": "SingleClick", "action": "Return" } ] }"#,
        )
        .unwrap();
    // 坏文件不应影响其它配置加载
    std::fs::write(user_dir.join("broken.json"), "{ not json").unwrap();
    // 非 json 文件应被忽略
    std::fs::write(user_dir.join("readme.txt"), "ignore me").unwrap();

    let registry = ProfileRegistry::load(Some(&user_dir));
    assert_eq!(registry.profiles().len(), 1, "坏文件和 txt 都不该进来");
    let hit = registry.match_process("MyApp.exe").expect("用户配置应命中");
    assert_eq!(hit.display_name(), "我的应用");
    assert_eq!(hit.bindings.len(), 1);
    assert_eq!(hit.id, "my-app");
}

#[test]
fn missing_user_dir_is_not_an_error() {
    let registry = ProfileRegistry::load(Some(Path::new(r"Z:\nope\nothing\here")));
    assert!(
        registry.profiles().is_empty(),
        "目录不存在（还没播种，或用户把配置删光了）就是空注册表"
    );
}

#[test]
fn seed_user_dir_materializes_seeds_once() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();

    let written = seed_user_dir(config_dir);
    assert_eq!(written, seed_templates().len(), "首次运行应把所有种子落地");
    assert!(config_dir.join(USER_PROFILE_DIR).is_dir());
    assert!(config_dir.join(SEED_MANIFEST).is_file());

    // 落地的文件能被正常加载，且 id 就是文件名。
    let user_dir = config_dir.join(USER_PROFILE_DIR);
    let registry = ProfileRegistry::load(Some(&user_dir));
    assert_eq!(registry.profiles().len(), seed_templates().len());
    for (id, _) in seed_templates() {
        assert!(registry.get_by_id(&id).is_some(), "缺少 {id}");
    }

    // 第二次运行不该重复写。
    assert_eq!(seed_user_dir(config_dir), 0, "已落地的种子不该再写一遍");
}

#[test]
fn seed_user_dir_never_resurrects_a_deleted_profile() {
    // 这条是整套种子机制存在的理由：用户删掉的配置不能在下次启动时自己回来。
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();
    seed_user_dir(config_dir);

    let Some((id, _)) = seed_templates().first().cloned() else {
        return;
    };
    let file = config_dir.join(USER_PROFILE_DIR).join(format!("{id}.json"));
    assert!(file.exists());
    std::fs::remove_file(&file).unwrap();

    // 模拟下次启动：清单里还记着这个 id，所以不会把它写回来。
    assert_eq!(seed_user_dir(config_dir), 0);
    assert!(!file.exists(), "删掉的配置不该在下次启动时复活");
    assert!(
        ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
            .get_by_id(&id)
            .is_none()
    );
}

#[test]
fn seed_user_dir_does_not_overwrite_an_edited_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();
    let Some((id, _)) = seed_templates().first().cloned() else {
        return;
    };
    let user_dir = config_dir.join(USER_PROFILE_DIR);
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::write(
        user_dir.join(format!("{id}.json")),
        r#"{ "process": "HandEdited.exe", "name": "手改过的" }"#,
    )
    .unwrap();

    seed_user_dir(config_dir);

    let registry = ProfileRegistry::load(Some(&user_dir));
    assert_eq!(
        registry.get_by_id(&id).unwrap().display_name(),
        "手改过的",
        "播种不能盖掉用户手改过的文件"
    );
}

#[test]
fn seed_user_dir_picks_up_seeds_added_later() {
    // 模拟「以后版本新增了一个种子」：清单里只记着旧的，新 id 不在其中。
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();
    let templates = seed_templates();
    if templates.len() < 2 {
        return;
    }
    let (first, second) = (templates[0].0.clone(), templates[1].0.clone());

    let user_dir = config_dir.join(USER_PROFILE_DIR);
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::write(config_dir.join(SEED_MANIFEST), format!(r#"["{first}"]"#)).unwrap();
    std::fs::write(user_dir.join(format!("{first}.json")), "{}").unwrap();

    seed_user_dir(config_dir);

    assert!(
        user_dir.join(format!("{second}.json")).exists(),
        "清单里没有的新种子应该自动落地"
    );
    let manifest = std::fs::read_to_string(config_dir.join(SEED_MANIFEST)).unwrap();
    assert!(
        manifest.contains(&first) && manifest.contains(&second),
        "两个 id 都该记进清单：{manifest}"
    );
}

#[test]
fn seed_user_dir_survives_a_lost_manifest() {
    // 清单丢了不该导致数据丢失：文件还在就只补记，不重写。
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();
    seed_user_dir(config_dir);

    let Some((id, _)) = seed_templates().first().cloned() else {
        return;
    };
    let file = config_dir.join(USER_PROFILE_DIR).join(format!("{id}.json"));
    std::fs::write(&file, r#"{ "process": "Mine.exe", "name": "我的" }"#).unwrap();
    std::fs::remove_file(config_dir.join(SEED_MANIFEST)).unwrap();

    seed_user_dir(config_dir);

    assert_eq!(
        ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
            .get_by_id(&id)
            .unwrap()
            .display_name(),
        "我的",
        "清单丢了也不能覆盖已存在的文件"
    );
    assert!(config_dir.join(SEED_MANIFEST).is_file(), "清单应被补回来");
}

#[test]
fn match_context_falls_back_to_window_title() {
    let mut registry = ProfileRegistry::default();
    let mut dsh = profile("__not_a_real_process__.exe", "DeepSeek Harness");
    dsh.window_title_contains = vec!["DeepSeek Harness".into()];
    registry.upsert(dsh);

    // 进程名认不出来，靠标题命中（WSL 里的服务就是这个情形）。
    assert_eq!(
        registry
            .match_context("chrome.exe", "打招呼 · DeepSeek Harness")
            .map(|p| p.display_name()),
        Some("DeepSeek Harness".to_string())
    );
    // 标题不匹配就不该命中。
    assert!(registry.match_context("chrome.exe", "别的标签页").is_none());
    // 空标题不能命中任何配置。
    assert!(registry.match_context("chrome.exe", "  ").is_none());
}

#[test]
fn match_context_prefers_process_over_title() {
    let mut registry = ProfileRegistry::default();
    let mut by_process = profile("Editor.exe", "编辑器");
    by_process.window_title_contains = vec!["不该赢".into()];
    registry.upsert(by_process);
    let mut by_title = profile("__not_a_real_process__.exe", "标题命中");
    by_title.window_title_contains = vec!["编辑器窗口".into()];
    registry.upsert(by_title);

    // 进程名和标题同时命中不同配置时，进程名优先。
    assert_eq!(
        registry
            .match_context("Editor.exe", "编辑器窗口")
            .map(|p| p.display_name()),
        Some("编辑器".to_string())
    );
}

#[test]
fn menu_entries_skip_profiles_without_icon() {
    let mut registry = ProfileRegistry::default();
    registry.upsert(profile("A.exe", "无图标"));
    let mut with_icon = profile("B.exe", "有图标");
    with_icon.icon = Some(IconSpec {
        label: "B".into(),
        color: "#3B82F6".into(),
    });
    registry.upsert(with_icon);
    let mut blank = profile("C.exe", "空图标");
    blank.icon = Some(IconSpec {
        label: "   ".into(),
        color: "#000".into(),
    });
    registry.upsert(blank);

    let entries = registry.menu_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].display_name(), "有图标");
}

#[test]
fn slug_is_ascii_and_handles_paths_and_lists() {
    assert_eq!(slug("ZCode.exe"), "zcode");
    assert_eq!(slug("  zcode.EXE  "), "zcode");
    assert_eq!(slug(r"C:\Program Files\ZCode\ZCode.exe"), "zcode");
    assert_eq!(slug("My App.exe"), "my-app");
    assert_eq!(slug("A.exe, B.exe"), "a");
    // 中文进程名不能原样当文件名：字符全被丢掉，退化成安全兜底值。
    assert_eq!(slug("我的助手.exe"), "app");
    assert_eq!(slug(""), "app");
    assert_eq!(slug("   "), "app");
}

#[test]
fn new_profile_id_avoids_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();

    let first = new_profile_id(config_dir, "ZCode.exe");
    assert_eq!(first, "zcode");

    let mut p = profile("ZCode.exe", "ZCode");
    p.id = first.clone();
    save_profile(config_dir, &p).unwrap();

    let second = new_profile_id(config_dir, "ZCode.exe");
    assert_eq!(second, "zcode-2", "撞名要加后缀，不能覆盖已有的那份");

    // 没有进程名的纯标题配置退化成 app / app-1……
    assert_eq!(new_profile_id(config_dir, ""), "app");
}

#[test]
fn save_then_load_round_trips_and_keeps_id_out_of_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();

    let mut p = profile("MyApp.exe", "我的应用");
    p.id = "my-app".into();
    p.note = "备注".into();
    save_profile(config_dir, &p).unwrap();

    let raw =
        std::fs::read_to_string(config_dir.join(USER_PROFILE_DIR).join("my-app.json")).unwrap();
    assert!(!raw.contains("\"id\""), "id 不该出现在文件里：{raw}");

    let registry = ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)));
    let back = registry.get_by_id("my-app").expect("应能读回来");
    assert_eq!(back.display_name(), "我的应用");
    assert_eq!(back.bindings.len(), 1);

    // 原子写入不该留下临时文件被当成配置读进来。
    assert_eq!(registry.profiles().len(), 1);
    assert!(!config_dir
        .join(USER_PROFILE_DIR)
        .join("my-app.json.tmp")
        .exists());
}

#[test]
fn save_profile_rejects_unsafe_ids() {
    let dir = tempfile::tempdir().unwrap();
    for bad in ["../escape", r"..\escape", "a/b", "", "有中文"] {
        let mut p = profile("X.exe", "X");
        p.id = bad.into();
        assert!(
            save_profile(dir.path(), &p).is_err(),
            "{bad:?} 不该被当成合法 id"
        );
    }
    assert!(remove_profile(dir.path(), "../../config").is_err());
}

#[test]
fn remove_profile_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();

    let mut p = profile("MyApp.exe", "我的应用");
    p.id = "my-app".into();
    save_profile(config_dir, &p).unwrap();
    assert!(
        ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
            .get_by_id("my-app")
            .is_some()
    );

    remove_profile(config_dir, "my-app").unwrap();
    assert!(
        ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
            .get_by_id("my-app")
            .is_none()
    );

    // 再删一次不该报错——用户要的结果是「它没了」，而它确实没了。
    remove_profile(config_dir, "my-app").unwrap();
}

#[test]
fn removing_a_seeded_id_does_not_bring_it_back() {
    // 编辑器和种子机制的交界处：删掉一份落地过的种子配置，重启不能复活它。
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();
    seed_user_dir(config_dir);

    let Some((id, _)) = seed_templates().first().cloned() else {
        return;
    };
    remove_profile(config_dir, &id).unwrap();

    assert_eq!(seed_user_dir(config_dir), 0, "删掉的种子不该被重新落地");
    assert!(
        ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
            .get_by_id(&id)
            .is_none()
    );
}
