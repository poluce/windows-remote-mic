import { useEffect, useRef, useState } from "react";
import { Sidebar, type PageId } from "./components/Sidebar";
import { ConnectionPage } from "./pages/ConnectionPage";
import { MappingPage } from "./pages/MappingPage";
import { SettingsPage } from "./pages/SettingsPage";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import { LogPage } from "./pages/LogPage";
import { OnboardingPage } from "./pages/OnboardingPage";
import { initRuntimeStatus } from "./store/runtimeStatus";
import { initUpdateStatus } from "./store/updateStatus";
import "./App.css";

/**
 * 铺满可视区的页面：内容自己分配高度，不出现整页滚动条。
 *
 * 只有按键映射页在这里——它主体是一张矩阵，滚动一下就等于看不全，
 * 「哪些键还没绑」这件事必须一屏能看到。其它页是竖排的设置/说明，
 * 天生比一屏长，滚动是对的。
 */
const FILL_PAGES: PageId[] = ["mapping"];

function App() {
  const [page, setPage] = useState<PageId>("connection");
  // 滚动容器是 .main（不是 .content）：这样滚动条贴窗口右缘，
  // 内容列在宽窗口里居中，不会把空白留在右侧。
  const scrollRef = useRef<HTMLElement | null>(null);

  // 切页时回到页面顶部：内容区是同一个滚动容器，不复位的话
  // 从长页切到另一页会停在中间，看起来像"页面没加载完"。
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
  }, [page]);

  useEffect(() => {
    // 全局运行时状态初始化：事件监听不随页面切换而销毁。
    initRuntimeStatus();
    // 更新状态同样全局初始化：正式构建下会自动静默检查一次。
    initUpdateStatus();
  }, []);

  return (
    <div className="app-shell">
      <Sidebar page={page} onChange={setPage} />

      <main className="main" ref={scrollRef}>
        <div
          className={`content${FILL_PAGES.includes(page) ? " content-fill" : ""}`}
        >
          {page === "connection" && <ConnectionPage />}
          {page === "mapping" && <MappingPage />}
          {page === "settings" && <SettingsPage />}
          {page === "diagnostics" && <DiagnosticsPage />}
          {page === "log" && <LogPage />}
          {page === "guidance" && <OnboardingPage />}
        </div>
      </main>
    </div>
  );
}

export default App;
