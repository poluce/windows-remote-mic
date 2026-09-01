---
name: hogp-report-path-re
description: 逆向 Windows HOGP 驱动（microsoft.bluetooth.profiles.hidovergatt.dll）定位遥控器 HID 报告的真实转发/写入点，并在驱动更新导致注入失效后重新找到新的拦截点。适用于「返回/音量键旁路失效」「吃掉模式失效」「Frida 注入点失效」等排查。
---

# HOGP 报告路径逆向与拦截点重定位

## 适用场景

- Remote Mic 的 Frida 旁路（`crates/core-hid/src/rc003_hid_tap.js`）注入失效：按键吃不到、系统仍响应、或 WUDFHost 崩溃。
- Windows 更新 / 驱动更新后，`microsoft.bluetooth.profiles.hidovergatt.dll` 的代码偏移变化，旧的 hook 地址（如 `0x20080`）不再指向报告拷贝点。
- 需要重新找到「报告从 GATT 通知到内核 HID 驱动」这条链路上的真实写入点。

## 背景：已探明的报告数据流（务必先理解）

遥控器按键的真实路径（2026-09 实测，Windows 11 26100）：

```
遥控器按键
  → BLE GATT 通知到达 WUDFHost.exe 中的 HOGP 驱动
  → 驱动把报告转成 8 字节队列项（4×u16 usages），放入环形队列
  → 处理函数 0x20720 取出队列项
  → 0x1febc 调 WUDFx02000.dll+0x1aab0 分配 WDF 请求缓冲区
  → 0x20080 的 memcpy 把队列项拷进请求缓冲区   ← 真正的写入点
  → WUDFx02000.dll+0xcd90 完成 WDF 请求（information=报告长度）
  → 内核 HID 驱动读取该缓冲区 → Raw Input / 系统响应
  → 处理完后驱动再发 READ_CHARACTERISTIC_IOCTL(0x80018483) 作为「补读」
```

关键结论（都是踩坑换来的）：

1. **`READ_CHARACTERISTIC_IOCTL = 0x80018483` 不是报告来源**。它是报告处理完之后才发出的「补读/续读」，输出缓冲区实测返回全零（GATT 通知已把特征值消费掉）。**清零这个 IOCTL 的输出缓冲区永远无效**——旧 eat 模式失败的根本原因。
2. **报告走 GATT 通知 → 队列项**，系统真正消费的是队列项经 memcpy 拷入的 WDF 请求缓冲区。
3. 队列项格式是 **8 字节 = 4×u16 usages**（如 `00 00 52 00 00 00 00 00` 表示上键 0x0052 在第 2 槽），与 IOCTL 输出的 9 字节格式（`01 00 00` + 3×u16）不同。
4. 驱动用**编码指针**保存 WDF 对象：真实指针 = `~encoded & ~7`，对象头 `+0x10` 处有类型魔数（0x1003 / 0x1008）。

## 当前版本的关键偏移（驱动更新后全部要重新找）

`microsoft.bluetooth.profiles.hidovergatt.dll`（base 运行时获取）：

| 偏移 | 含义 |
| --- | --- |
| `+0x20720` | 报告处理函数（环形队列出队 → 转发 → 补读） |
| `+0x1febc` | 报告拷贝/分配函数（调 `[A+0x548]` 分配缓冲区） |
| `+0x20080` | **memcpy 调用点 = 真实写入点**（dst=转发缓冲区, src=队列项） |
| `+0x2097f` | 间接调用点 1：`[A+0x2d8]` 创建请求对象 |
| `+0x20aa2` | 间接调用点 2：`[A+0x580]` 错误路径 |
| `+0x20b4c` | 间接调用点 3：`[A+0x520]` 完成请求（触发补读 IOCTL） |
| `+0x28010` | 间接调用 thunk：`jmp [0x2a168]` → `jmp rax`（rax=目标函数） |
| `+0x246d0` | memcpy 导入 thunk：`jmp [0x29ff0]` |
| `+0x32dc0` | 全局 A：函数表/接口指针（槽位见下） |
| `+0x32dc8` | 全局 B：WDF 对象（COM 接口在对象 +0x1c0 处） |

函数表槽位（A 指向 WUDFx02000.dll 的接口表）：

| 槽位 | 目标 | 作用 |
| --- | --- | --- |
| `+0x2d8` | WUDFx02000.dll+0x8ff0 | 创建请求对象（输入模板对象，输出编码指针） |
| `+0x520` | WUDFx02000.dll+0xcd90 | 完成请求（status, information=报告长度） |
| `+0x548` | WUDFx02000.dll+0x1aab0 | 为请求分配缓冲区（size, &buf, &handle） |
| `+0x580` | WUDFx02000.dll+0x78de0 | 错误路径处理 |

## 本次成功的定位方法论（按顺序执行）

### 第 1 步：从 IOCTL 钩子拿驱动侧返回地址

- 钩 `NtDeviceIoControlFile`，只捕获 `READ_CHARACTERISTIC_IOCTL` 且 `STATUS_SUCCESS`、输出长度 9 的情况。
- 用 `Thread.backtrace(context, Backtracer.ACCURATE)` 找**第一个落在驱动模块内的返回地址**（`findDriverFrame`）。
- 注意：直接看 `this.returnAddress` 只会看到 kernel32 的 `DeviceIoControl` 包装，必须走 backtrace。

### 第 2 步：静态反汇编驱动 DLL

- DLL 位置：`C:\Windows\System32\DriverStore\FileRepository\hidbthle.inf_*\Microsoft.Bluetooth.Profiles.HidOverGatt.dll`。
- 拷到 WSL 用 `objdump -d -M intel` 反汇编（PE 格式直接支持）。
- 从返回地址向前找函数头（`int3` 填充之后），向后读完整函数。
- 识别特征：环形队列出队（`[rdi+0x28/0x30/0x38/0x40]` 的 mask/索引运算）、WPP 追踪调用（`[rsp+0x20]=level, [rsp+0x30]=line`）、`call 0x28010` 间接调用、`call 0x246d0` memcpy。

### 第 3 步：识别「分配 → 拷贝 → 完成」三段式

报告处理函数里必然出现：

1. 一次间接调用创建/格式化请求（输出一个编码指针到栈局部变量）；
2. 一个 `memcpy(dst, src, len)`，src 是队列项缓冲区，len 是 8（或 8+1）；
3. 一次间接调用完成请求（最后一个参数是报告长度）。

**memcpy 的 src 就是拦截点**：在 memcpy 执行前清零 src，转发缓冲区自然得到全零，系统看不到按键。

### 第 4 步：运行时验证（只钩 call 指令/函数入口，绝不钩 jmp thunk）

在 Frida 脚本里对**具体 call 指令地址**和**函数入口**挂 `Interceptor.attach`：

```js
// 函数入口：读队列项报告
Interceptor.attach(driver.base.add(0x1febc), {
  onEnter(args) {
    const begin = args[3].readPointer();
    const end = args[3].add(8).readPointer();
    const len = end.sub(begin).toUInt32();
    // 上报 hex(begin, len)
  },
});
// memcpy 调用点：记录 + 吃掉
Interceptor.attach(driver.base.add(0x20080), {
  onEnter(args) {
    const dst = args[0], src = args[1], len = args[2].toUInt32();
    // 先记录原始 hex，再清零 src（eat 模式）
    for (let i = 0; i < len; i++) src.add(i).writeU8(0);
  },
});
// 间接调用点：rax 是目标函数
Interceptor.attach(driver.base.add(0x20b4c), {
  onEnter(args) {
    const target = this.context.rax; // 解析到模块+偏移
  },
});
```

### 第 5 步：dump 全局函数表识别框架方法

读 `A=[base+0x32dc0]`，遍历 `A+0x000..A+0x600` 每个非空指针，用 `Process.findModuleByAddress` 解析到 `模块+偏移`。再对照 WUDFx02000.dll 的 pdata/反汇编确认方法语义（编码指针解码、魔数检查、完成请求等）。

### 第 6 步：二进制验证（有/没有，不要用多/少）

- 开启 eat 后按「上」：日志必须**没有** `[raw_input] 遥控器键盘事件`，且出现「吃掉报告」。
- 关闭 eat 再按：`[raw_input]` 必须出现。
- 同时确认旁路连接不断开、WUDFHost 不崩（查事件日志 Application 里 WUDFHost 的 1000/1001）。

## 遇到的问题与规避点（血泪清单）

| 问题 | 现象 | 规避 |
| --- | --- | --- |
| 清零 IOCTL 输出缓冲无效 | 系统仍响应按键 | 先搞清数据流再选拦截点；补读 IOCTL 的输出不是系统消费的数据 |
| 钩 ucrtbase 的 memcpy 抓不到驱动调用 | 无 memcpy 日志 | API-set 转发器隔在中间，返回地址不在驱动模块；改钩驱动自己的 thunk 或 call 指令 |
| **钩 jmp thunk（0x28010/0x246d0）导致 WUDFHost fastfail** | 注入后 3~5 秒 WUDFHost 崩溃（0xc0000409，frida-gadget.dll+0x1006a2d），应用反复弹 UAC | **绝不 `Interceptor.attach` jmp 指令**；只钩函数入口和 call 指令 |
| jmp thunk 上挂 onLeave | 同上崩溃 | 同上；onLeave 会压返回地址，被调函数抛 C++ 异常时栈展开直接炸 |
| call 指令上挂 onLeave | onLeave 不触发/不可靠 | 需要「拷贝后清零」时改为 **onEnter 里清零 src**，让 memcpy 自然拷零 |
| JS 发中文消息写坏 socket | Rust 端报 `stream did not contain valid UTF-8`，旁路反复断开重连 | `charCodeAt & 0xff` 会截断中文；emit 必须做真正的 UTF-8 编码 |
| 反汇编只看到 kernel32 包装 | 找不到驱动代码 | 用 `Thread.backtrace` 找驱动模块帧，不要用 `this.returnAddress` |
| 扫描 IOCTL 常量 0x80018483 无结果 | 常量不是立即数 | 别指望字节扫描；从 backtrace 定位调用点 |
| 杀掉 WUDFHost 后宿主不重生 | 旁路一直「未找到 HostPid」 | 遥控器断电重开 + 应用重连 |
| 应用重启后跳过注入（旧脚本还在跑） | 改了脚本不生效 | 读 `%PROGRAMDATA%\RemoteMic\hid-tap\injected-pid.txt`，手动 taskkill 该 PID（需 UAC）再重连 |
| 宿主崩溃循环导致反复弹 UAC | 用户被弹窗轰炸 | 先修脚本再注入；`INJECT_ATTEMPTED_PID` 只挡同一 PID，宿主一换又会弹 |
| WDF 对象指针看不懂 | 参数像内核地址（0xffff…） | 是编码指针：`~ptr & ~7` 解码，`+0x10` 有魔数 0x1003/0x1008 |
| 队列项格式与 IOCTL 输出不同 | 8 字节 vs 9 字节 | 队列项 = 4×u16 usages（首槽常为 0）；IOCTL 输出 = `01 00 00` + 3×u16 |

## 驱动更新后重新定位的 Playbook

1. **确认失效**：开 trace 按「上」，看日志里 `报告处理`/`memcpy 追踪` 是否还出现、`[raw_input]` 是否又回来了。
2. **重新拿返回地址**：IOCTL 钩子 + backtrace 找新驱动里的调用帧（`findDriverFrame` 逻辑不变）。
3. **重新反汇编**：从 DriverStore 拷新 DLL，objdump 找新函数。老特征仍然有效：
   - 环形队列（`[rdi+0x28/0x30/0x38/0x40]`）；
   - WPP 追踪（`[rsp+0x20]=level`、`[rsp+0x30]=line`）；
   - `call <间接调用 thunk>` 三段式（创建请求 → memcpy → 完成请求）；
   - memcpy 的 src 是队列项、len=8。
4. **重新 dump 函数表**：读新 `A/B` 全局（偏移可能变，先找 `lea r8,[rip+…]` 引用 `B` 的初始化代码），解析 WUDFx 槽位。
5. **只钩 call 指令/函数入口验证**，确认新 memcpy 调用点后，把 eat 清零逻辑搬过去。
6. **二进制验证**：eat 开 → 无 `[raw_input]`；eat 关 → 有 `[raw_input]`；连接不断、宿主不崩。

## 已知缺口与闭环方案

- **eat 开启后，应用自己的 Raw Input 也收不到信号**：清零发生在驱动转发之前，系统与应用共用的 Raw Input 通道一起被切断，映射动作不会触发。
- **闭环方案（已实现，待真机验证）**：在 `0x20080` 的 onEnter 里，**先**把清零前的队列项原始报告转成 9 字节 IOCTL 格式（`01 00 00` + 队列项字节 2..8 的 3 个 usage），以 `gatt_read` 消息发给应用调度器，**再**清零 src。这样系统看不到按键，应用照常注入映射动作。
- 转换函数（实测对应关系：上键队列项 `00 00 52 00 00 00 00 00` ↔ IOCTL `01 00 00 52 00 00 00 00 00`）：

```js
function queueToIoctlHex(src, len) {
  if (len !== 8) return "";
  try {
    const bytes = new Uint8Array(src.readByteArray(8));
    let out = "010000";
    for (let i = 2; i < 8; i++) {
      out += bytes[i].toString(16).padStart(2, "0");
    }
    return out;
  } catch (_e) {
    return "";
  }
}
```

- 验证标准：eat 开 → 系统无 `[raw_input]`，但应用日志出现 `HOGP 载荷 usage: 0x0052` 且映射动作执行；eat 关 → 系统有 `[raw_input]`。

## 附录：可复用代码模板（本次验证可用的完整骨架）

以下代码来自 `crates/core-hid/src/rc003_hid_tap.js`，驱动更新后只需改偏移常量。

```js
// ---- 基础工具 ----
function hex(pointer, length) {
  if (pointer.isNull() || length <= 0) return "";
  const bytes = new Uint8Array(pointer.readByteArray(length));
  let result = "";
  for (let i = 0; i < bytes.length; i++) {
    result += bytes[i].toString(16).padStart(2, "0");
  }
  return result;
}

function resolveModule(addr) {
  if (addr.isNull()) return "null";
  try {
    const m = Process.findModuleByAddress(addr);
    if (m) return m.name + "+0x" + addr.sub(m.base).toString(16);
  } catch (_e) {}
  return addr.toString();
}

function readPointer(addr) {
  try { return addr.readPointer(); } catch (_e) { return ptr(0); }
}

// emit 必须用真正的 UTF-8 编码（charCodeAt & 0xff 会截断中文，
// 写坏 socket，Rust 端报 stream did not contain valid UTF-8）
function utf8Bytes(text) {
  const out = [];
  for (let i = 0; i < text.length; i++) {
    let c = text.charCodeAt(i);
    if (c < 0x80) {
      out.push(c);
    } else if (c < 0x800) {
      out.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
    } else if (c >= 0xd800 && c <= 0xdbff && i + 1 < text.length) {
      const d = text.charCodeAt(i + 1);
      if (d >= 0xdc00 && d <= 0xdfff) {
        c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00);
        i++;
        out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 0x3f),
                 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
      } else {
        out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
      }
    } else {
      out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
    }
  }
  return out;
}

// ---- 驱动帧定位 ----
function findDriverFrame(context) {
  const driver = Process.findModuleByName(
    "microsoft.bluetooth.profiles.hidovergatt.dll");
  if (!driver) return null;
  try {
    const bt = Thread.backtrace(context, Backtracer.ACCURATE);
    for (const addr of bt) {
      if (addr.compare(driver.base) >= 0 &&
          addr.compare(driver.base.add(driver.size)) < 0) {
        return addr;
      }
    }
  } catch (_e) {}
  return null;
}

// ---- 全局对象与函数表 ----
function readGlobalAB() {
  const driver = Process.findModuleByName(
    "microsoft.bluetooth.profiles.hidovergatt.dll");
  if (!driver) return { a: ptr(0), b: ptr(0) };
  return {
    a: readPointer(driver.base.add(0x32dc0)), // 函数表/接口指针
    b: readPointer(driver.base.add(0x32dc8)), // WDF 对象
  };
}

function dumpGlobalTable() {
  const { a, b } = readGlobalAB();
  const table = [];
  if (!a.isNull()) {
    for (let off = 0; off < 0x600; off += 8) {
      const p = readPointer(a.add(off));
      if (!p.isNull()) {
        table.push("+0x" + off.toString(16) + ": " + p + " -> " + resolveModule(p));
      }
    }
  }
  // 上报 table，对照 WUDFx02000.dll 反汇编识别每个槽位的方法语义
}

// ---- 报告路径钩子（核心，只钩函数入口和 call 指令）----
function installReportPathHooks() {
  const driver = Process.findModuleByName(
    "microsoft.bluetooth.profiles.hidovergatt.dll");
  if (!driver) return;

  // 1) 报告处理函数入口：r9 指向队列项 {begin,end}，报告在 [begin,end)
  Interceptor.attach(driver.base.add(0x1febc), {
    onEnter(args) {
      const begin = args[3];
      if (begin.isNull()) return;
      try {
        const end = begin.readPointer();
        const endPtr = args[3].add(8).readPointer();
        const len = endPtr.sub(end).toUInt32();
        if (len >= 1 && len <= 64) {
          emit({ kind: "report_path", src: end.toString(),
                 len: len, hex: hex(end, Math.min(len, 16)) });
        }
      } catch (_e) {}
    },
  });

  // 2) memcpy 调用点 = 真实写入点：dst=转发缓冲区, src=队列项
  Interceptor.attach(driver.base.add(0x20080), {
    onEnter(args) {
      const dst = args[0], src = args[1], len = args[2].toUInt32();
      if (len < 1 || len > 64 || src.isNull() || dst.isNull()) return;
      // 先记录原始报告
      emit({ kind: "memcpy_trace", dst: dst.toString(), src: src.toString(),
             len: len, hex: hex(src, Math.min(len, 16)) });
      if (!eatEnabled) return;
      // 闭环：先把原始报告转 9 字节格式发给应用调度，再清零 src
      const ioctlHex = queueToIoctlHex(src, len);
      if (ioctlHex) emit({ kind: "gatt_read", raw: ioctlHex, eaten: true });
      try {
        for (let i = 0; i < len; i++) src.add(i).writeU8(0);
        emit({ kind: "eaten_report", message: "eaten len=" + len });
      } catch (_e) {}
    },
  });

  // 3) 三个框架间接调用点：rax 是目标函数（仅追踪用）
  for (const off of [0x2097f, 0x20aa2, 0x20b4c]) {
    Interceptor.attach(driver.base.add(off), {
      onEnter(args) {
        const target = this.context.rax;
        emit({ kind: "vcall_trace", target: target.toString(),
               target_resolved: resolveModule(target),
               args: ["rcx=" + args[0], "rdx=" + args[1],
                      "r8=" + args[2], "r9=" + args[3]] });
      },
    });
  }
}
```

**使用提醒**：所有偏移（`0x1febc`/`0x20080`/`0x2097f`/`0x20aa2`/`0x20b4c`/`0x32dc0`/`0x32dc8`）都是当前驱动版本的 RVA；驱动更新后按 Playbook 重新定位，代码结构不用改。

## 操作环境速查

- 日志：`%LOCALAPPDATA%\RemoteMic\RC003\remote-mic.log`（WSL 路径 `/mnt/c/Users/<用户>/AppData/Local/RemoteMic/RC003/remote-mic.log`）。
- 环境变量：`REMOTE_MIC_HID_TAP_EAT=1`、`REMOTE_MIC_HID_TAP_TRACE=1`（tauri dev 启动时设置）。
- 注入状态文件：`%PROGRAMDATA%\RemoteMic\hid-tap\injected-pid.txt`。
- 杀 WUDFHost 需 UAC：`Start-Process taskkill -ArgumentList '/PID','<pid>','/F' -Verb RunAs -Wait`。
- 崩溃取证：事件查看器 Application 日志，WUDFHost 1000/1001，异常 0xc0000409 + frida-gadget.dll 偏移 = fastfail。
- 每次改 `rc003_hid_tap.js` 后：tauri dev 自动重建 → 杀旧 WUDFHost → 遥控器断电重开 → 应用重连 → 按「上」验证。
