export type MappingEntry = {
  button: string;
  name: string;
  trigger: string;
  action: string;
  action_key: string;
};

export type RemoteButton = {
  key: string;
  name: string;
};

export type TriggerOption = {
  key: string;
  label: string;
  desc: string;
};

export type ActionItem = {
  key: string;
  label: string;
};

export type ActionCategory = {
  key: string;
  title: string;
  actions: ActionItem[];
};

/** 一份应用专属配置。id 是它在 `app-profiles/` 里的文件名，也是保存/删除的目标。 */
export type AppProfileView = {
  id: string;
  name: string;
  /** 展示用的进程名写法；纯标题配置为空串。 */
  process: string;
  title_contains: string[];
  /** 快捷菜单内圈用的品牌色；没配图标时为 null。 */
  icon_color: string | null;
  note: string;
  /** 这份配置覆盖了哪些 (按键, 触发)；没列出的格子沿用全局。 */
  bindings: MappingEntry[];
};

/** 作用范围：全局，或某一份应用配置的 id。 */
export type ScopeId = string;

/**
 * 全局作用域的哨兵 id。
 *
 * 用带下划线的写法是**刻意的**：后端的 `new_profile_id` 把进程名 slug 成
 * `[a-z0-9-]`，永远产不出下划线，所以任何一个真实配置的 id 都不可能等于它。
 * 直接叫 `"global"` 就不行了——真有个 `global.exe`，它的 id 就正好是 `global`，
 * 那份配置会在界面上被全局挡掉，永远选不中。
 */
export const GLOBAL_SCOPE: ScopeId = "__global__";