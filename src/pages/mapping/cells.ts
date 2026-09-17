import type { MappingEntry } from "./types";

/** 矩阵里一个格子的坐标：按键 + 触发方式。 */
export function cellKey(button: string, trigger: string): string {
  return `${button}|${trigger}`;
}

/**
 * 把扁平的绑定列表转成按格子索引的 Map。
 *
 * 全局映射和某份应用配置的覆盖层用的是同一套键，所以矩阵可以拿两份 Map 直接
 * 对同一个格子取值——这也是「覆盖层」的语义：格子先问配置，配置没有才落到全局。
 */
export function toCellMap(entries: MappingEntry[]): Map<string, MappingEntry> {
  const map = new Map<string, MappingEntry>();
  for (const entry of entries) {
    map.set(cellKey(entry.button, entry.trigger), entry);
  }
  return map;
}
