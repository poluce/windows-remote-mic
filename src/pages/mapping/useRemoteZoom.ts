import { useEffect, type RefObject } from "react";

/**
 * 遥控器图形按卡片剩下的高度缩放。
 *
 * 它是定尺寸的像素图（105×345，内部全是绝对定位的按键），没法用百分比或
 * flex 压缩，只能整体 zoom。而它默认放大 1.3 倍后有 448px 高——这是整页
 * 704px 底线的来源：窗口一矮，卡片压不下去，滚动条就出来了。
 *
 * 所以量出 `.remote-art` 实际分到多高，反推 zoom。这样窗口怎么拉都不会把
 * 遥控器挤出去，也不会让它把整页顶高。
 */
export function useRemoteZoom(containerRef: RefObject<HTMLDivElement | null>) {
  useEffect(() => {
    const box = containerRef.current;
    if (!box) return;
    const BASE_HEIGHT = 345; // 遥控器未缩放的原始高度，见 Xiaomi2ProRemote.css
    const apply = () => {
      const zoom = Math.min(1.3, Math.max(0.4, box.clientHeight / BASE_HEIGHT));
      box.style.setProperty("--remote-zoom", zoom.toFixed(3));
    };
    // 先同步量一次：只靠 ResizeObserver 的话，第一帧之前遥控器一直是
    // 未缩放的 448px，会先闪一下、也可能正好卡在「量不到」的状态。
    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(box);
    return () => observer.disconnect();
  }, [containerRef]);
}
