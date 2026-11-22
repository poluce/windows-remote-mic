import { useEffect, type RefObject } from "react";

/**
 * 遥控器图形按卡片剩下的高度缩放，**整列宽度跟着缩放走**。
 *
 * 它是定尺寸的像素图（105×345，内部全是绝对定位的按键），没法用百分比或
 * flex 压缩，只能整体 zoom。以前 zoom 封顶 1.3（448px 高），窗口一高就比右边
 * 的向导 + 矩阵卡矮一截、上下各留一块空白——所以现在：
 *
 * 1. 量出 `.remote-art` 分到多高，反推 zoom（上限 2.0，再大右卡就没地方了）；
 * 2. 把左列宽度写回 `.mapping-wizard` 的 `--remote-col-width`，让这一列
 *    恰好等于「遥控器宽度 + 一点余量」——既不裁图形，也不白占宽度。
 *
 * 宽度按 8px 台阶取整：右卡变窄后文字可能多折一行、高度微变，量化之后
 * 这点抖动不会再反向推动宽度，避免缩放与列宽互相追着跑。
 */
export function useRemoteZoom(containerRef: RefObject<HTMLDivElement | null>) {
  useEffect(() => {
    const box = containerRef.current;
    if (!box) return;

    const BASE_HEIGHT = 345; // 遥控器未缩放的原始高度，见 Xiaomi2ProRemote.css
    const BASE_WIDTH = 105; // 同上，原始宽度
    // 上限约等于「右卡 540px 高」时的满高缩放；再往左卡就明显挤压映射表了。
    const MAX_ZOOM = 1.6;
    const MIN_ZOOM = 0.5;
    const COLUMN_GUTTER = 10;

    const apply = () => {
      const raw = box.clientHeight / BASE_HEIGHT;
      const zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, raw));
      box.style.setProperty("--remote-zoom", zoom.toFixed(3));

      const wizard = box.closest(".mapping-wizard");
      if (wizard instanceof HTMLElement) {
        const width = Math.round((BASE_WIDTH * zoom + COLUMN_GUTTER) / 8) * 8;
        const next = `${width}px`;
        if (wizard.style.getPropertyValue("--remote-col-width") !== next) {
          wizard.style.setProperty("--remote-col-width", next);
        }
      }
    };

    // 先同步量一次：只靠 ResizeObserver 的话，第一帧之前遥控器一直是
    // 未缩放的 448px，会先闪一下、也可能正好卡在「量不到」的状态。
    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(box);
    return () => observer.disconnect();
  }, [containerRef]);
}
