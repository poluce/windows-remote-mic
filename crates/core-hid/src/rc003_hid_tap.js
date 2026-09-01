// HOGP 报告探针。默认只读；开启「吃掉」模式后，在驱动真正的报告转发点
// （0x20080 的 memcpy 源缓冲区，即队列项）把 usage 清零，系统 HOGP 层
// 看不到任何遥控器按键；同时把清零前的原始报告转成 9 字节 IOCTL 格式
// 上报给应用，由应用注入映射动作（单一持有者）。
//
// 关键约束（与已验证可用的 remote-bridge-hub 实现一致）：
// - 只 hook 目标 IOCTL `READ_CHARACTERISTIC_IOCTL`(0x80018483)；
// - 只在返回 STATUS_SUCCESS 且输出长度恰为 9 字节时读取输出缓冲区。
// 不读取其它 IOCTL 的 args[8]：那可能是 MDL / 内核指针 / 已释放缓冲，
// 读它既无意义又有风险。
// - 不钩 jmp thunk（Frida 对 jmp 的拦截在 WUDFHost 里会 fastfail），
//   只钩函数入口与 call 指令。

const READ_CHARACTERISTIC_IOCTL = 0x80018483;
const EXPECTED_OUTPUT_LENGTH = 9;
const STATUS_SUCCESS = 0;
const STATUS_PENDING = 0x103;
const HEARTBEAT_MS = 5000;
const RECONNECT_MS = 1000;
const EAT_POLL_MS = 1000;

let host = "127.0.0.1";
let port = 17331;
// 是否开启「吃掉」模式：初始值由 Gadget config 的 parameters.eat 传入，
// 运行中每秒轮询 eat-mode.txt 热更新（设置页切换无需重新注入）。
let eatEnabled = false;
let eatModePath = "C:\\ProgramData\\RemoteMic\\hid-tap\\eat-mode.txt";
// 是否开启 IOCTL/模块追踪：由 Gadget config 的 parameters.trace 传入。
let traceEnabled = false;
// 保持 SocketConnection 引用，防止 QuickJS GC 提前关闭 socket。
let connection = null;
let output = null;
let connecting = false;
let reconnectTimer = null;
let hookInstalled = false;
let globalDumped = false;

// 串行写链：每次 writeAll 都排在上一次之后，避免并发写把 socket 写坏。
let writeChain = Promise.resolve();

// hook 统计，随心跳上报，用于确认 hook 是否持续被调用。
let stats = { calls: 0, captured: 0, success: 0, pending: 0, other_status: 0 };

function asciiBytes(text) {
  // 真正的 UTF-8 编码：之前按 charCodeAt & 0xff 截断，中文会变成非法
  // UTF-8 字节流，把旁路 socket 写坏（Rust 端报 stream did not contain
  // valid UTF-8 并断开重连）。
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
        out.push(
          0xf0 | (c >> 18),
          0x80 | ((c >> 12) & 0x3f),
          0x80 | ((c >> 6) & 0x3f),
          0x80 | (c & 0x3f)
        );
      } else {
        out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
      }
    } else {
      out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
    }
  }
  return out;
}

function hex(pointer, length) {
  if (pointer.isNull() || length <= 0) {
    return "";
  }
  const bytes = new Uint8Array(pointer.readByteArray(length));
  let result = "";
  for (let i = 0; i < bytes.length; i++) {
    result += bytes[i].toString(16).padStart(2, "0");
  }
  return result;
}

// 把地址解析成「模块名+偏移」，解析失败就返回原始地址字符串。
function resolveModule(addr) {
  if (addr.isNull()) {
    return "null";
  }
  try {
    const m = Process.findModuleByAddress(addr);
    if (m) {
      return m.name + "+0x" + addr.sub(m.base).toString(16);
    }
  } catch (_e) {}
  return addr.toString();
}

// 安全读指针：失败返回 null 指针，避免读坏地址拖垮脚本。
function readPointer(addr) {
  try {
    return addr.readPointer();
  } catch (_e) {
    return ptr(0);
  }
}

// 读驱动全局对象：A=[base+0x32dc0]（函数表/接口指针），B=[base+0x32dc8]（对象）。
function readGlobalAB() {
  const driver = Process.findModuleByName(
    "microsoft.bluetooth.profiles.hidovergatt.dll"
  );
  if (!driver) {
    return { a: ptr(0), b: ptr(0) };
  }
  return {
    a: readPointer(driver.base.add(0x32dc0)),
    b: readPointer(driver.base.add(0x32dc8)),
  };
}

// 一次性 dump 全局函数表：A 指向的表里每个非空槽位都解析到模块+偏移，
// 用于识别 [A+0x520]/[A+0x548]/[A+0x580] 到底是 WUDF 框架的哪个方法。
function dumpGlobalTable() {
  const { a, b } = readGlobalAB();
  const table = [];
  if (!a.isNull()) {
    for (let off = 0; off < 0x600; off += 8) {
      const p = readPointer(a.add(off));
      if (!p.isNull()) {
        table.push(
          "+0x" + off.toString(16) + ": " + p.toString() + " -> " + resolveModule(p)
        );
      }
    }
  }
  const bVtable = b.isNull() ? ptr(0) : readPointer(b);
  emit({
    kind: "global_info",
    a: a.toString(),
    b: b.toString(),
    a_resolved: resolveModule(a),
    b_resolved: resolveModule(b),
    b_vtable: bVtable.toString(),
    b_vtable_resolved: resolveModule(bVtable),
    table: table,
  });
}

// 枚举 WUDFHost 里 HID/BTH 相关模块及其导出函数，用于定位报告转发点。
function enumerateHidModules() {
  const modules = Process.enumerateModules();
  const result = [];
  for (const m of modules) {
    const name = m.name.toLowerCase();
    if (
      name.includes("hid") ||
      name.includes("bth") ||
      name.includes("wudf") ||
      name.includes("gatt")
    ) {
      const exports = [];
      try {
        for (const e of m.enumerateExports()) {
          if (e.name) {
            exports.push(e.name);
          }
        }
      } catch (_e) {}
      result.push({
        name: m.name,
        base: m.base.toString(),
        size: m.size,
        exports: exports,
      });
    }
  }
  return result;
}

// 枚举 HOGP 驱动与 UMDF 框架的导入模块，用于判断报告处理调用了哪些库。
function enumerateImports() {
  const result = [];
  const modules = Process.enumerateModules();
  for (const m of modules) {
    const name = m.name.toLowerCase();
    if (name.includes("hidovergatt") || name.includes("wudfx")) {
      const imports = [];
      try {
        for (const imp of m.enumerateImports()) {
          imports.push(imp.name);
        }
      } catch (_e) {}
      result.push({ name: m.name, imports: imports });
    }
  }
  return result;
}

// 报告转发路径钩子。不钩 jmp thunk（Frida 对 jmp 的拦截在 WUDFHost 里
// 会 fastfail），改为钩具体 call 指令与函数入口：
// - 0x1febc：报告处理函数入口，r9 指向队列项 {begin,end}，报告字节在 [begin,end)；
// - 0x20080：函数内 memcpy 调用点（dst=新分配缓冲区, src=报告, len=长度）；
// - 0x2097f / 0x20aa2 / 0x20b4c：三个框架间接调用点（rax=目标函数）。
// 吃掉模式挂在 0x20080：拷贝前把 src 清零，memcpy 自然把零拷进转发
// 缓冲区；同时把清零前的原始报告转成 9 字节格式上报应用调度。
function installReportPathHooks() {
  const driver = Process.findModuleByName(
    "microsoft.bluetooth.profiles.hidovergatt.dll"
  );
  if (!driver) {
    return;
  }
  function rangeOf(addr) {
    try {
      const range = Process.findRangeByAddress(addr);
      if (range) {
        return (
          range.base.toString() +
          " size=0x" +
          range.size.toString(16) +
          " prot=" +
          range.protection +
          (range.file ? " file=" + range.file.path : " anon")
        );
      }
    } catch (_e) {}
    return "";
  }
  // 报告处理函数入口：记录队列项里的报告字节（仅追踪模式）。
  if (traceEnabled) {
    try {
      Interceptor.attach(driver.base.add(0x1febc), {
        onEnter(args) {
          const begin = args[3];
          if (begin.isNull()) {
            return;
          }
          let end = ptr(0);
          let len = 0;
          try {
            end = begin.readPointer();
            const endPtr = args[3].add(8).readPointer();
            len = endPtr.sub(end).toUInt32();
          } catch (_e) {
            return;
          }
          if (len < 1 || len > 64) {
            return;
          }
          emit({
            kind: "report_path",
            func: "0x1febc",
            dst: "",
            src: end.toString(),
            len: len,
            hex: hex(end, Math.min(len, 16)),
            dst_range: "",
          });
        },
      });
    } catch (_e) {}
  }
  // 报告拷贝点：memcpy(dst, src, len)。dst 就是交给 WDF 请求、最终被内核
  // HID 驱动读取的转发缓冲区——真正的「写入点」。
  try {
    Interceptor.attach(driver.base.add(0x20080), {
      onEnter(args) {
        const dst = args[0];
        const src = args[1];
        const len = args[2].toUInt32();
        if (len < 1 || len > 64 || src.isNull() || dst.isNull()) {
          return;
        }
        if (traceEnabled) {
          emit({
            kind: "memcpy_trace",
            func: "0x20080",
            dst: dst.toString(),
            src: src.toString(),
            len: len,
            hex: hex(src, Math.min(len, 16)),
            dst_range: rangeOf(dst),
          });
        }
        if (!eatEnabled) {
          return;
        }
        // 先把清零前的原始报告转成 9 字节格式上报应用（应用据此注入
        // 映射动作），再清零源缓冲区，让系统 HOGP 层看不到按键。
        const ioctlHex = queueToIoctlHex(src, len);
        if (ioctlHex) {
          emit({ kind: "gatt_read", raw: ioctlHex, eaten: true });
        }
        try {
          for (let i = 0; i < len; i++) {
            src.add(i).writeU8(0);
          }
          emit({
            kind: "eaten_report",
            message: "已清零报告源缓冲区 len=" + len,
          });
        } catch (_e) {}
      },
    });
  } catch (_e) {}
  // 三个框架间接调用点：rax 是目标函数（仅追踪模式）。
  if (traceEnabled) {
    const vcallSites = [0x2097f, 0x20aa2, 0x20b4c];
    for (const off of vcallSites) {
      try {
        Interceptor.attach(driver.base.add(off), {
          onEnter(args) {
            const target = this.context.rax;
            emit({
              kind: "vcall_trace",
              target: target.toString(),
              target_resolved: resolveModule(target),
              ret: this.returnAddress.toString(),
              args: [
                "rcx=" + args[0].toString(),
                "rdx=" + args[1].toString(),
                "r8=" + args[2].toString(),
                "r9=" + args[3].toString(),
              ],
              out1: "",
              out2: "",
            });
          },
        });
      } catch (_e) {}
    }
  }
}

// 完整调用栈（带模块解析），用于看清 READ_CHARACTERISTIC_IOCTL 的
// 驱动侧调用链：谁调用了 0x20720 这个报告处理函数。
function fullBacktrace(context) {
  const frames = [];
  try {
    const bt = Thread.backtrace(context, Backtracer.ACCURATE);
    for (const addr of bt) {
      frames.push(addr.toString() + " " + resolveModule(addr));
    }
  } catch (_e) {}
  return frames;
}

// 追踪驱动模块内的 NtWriteFile 调用（报告可能经文件句柄转发）。
function installWriteFileTrace() {
  const driver = Process.findModuleByName(
    "microsoft.bluetooth.profiles.hidovergatt.dll"
  );
  if (!driver) {
    return;
  }
  const ntdll = Process.findModuleByName("ntdll.dll");
  const target = ntdll ? ntdll.findExportByName("NtWriteFile") : null;
  if (!target) {
    return;
  }
  try {
    Interceptor.attach(target, {
      onEnter(args) {
        const ret = this.returnAddress;
        if (
          ret.compare(driver.base) < 0 ||
          ret.compare(driver.base.add(driver.size)) >= 0
        ) {
          return;
        }
        const buffer = args[1];
        const length = args[2].toUInt32();
        if (length < 1 || length > 64 || buffer.isNull()) {
          return;
        }
        emit({
          kind: "writefile_trace",
          handle: args[0].toString(),
          len: length,
          hex: hex(buffer, Math.min(length, 16)),
        });
      },
    });
  } catch (_e) {}
}

// 反汇编指定地址开始的 count 条指令，用于分析驱动读特征后的处理代码。
function disassemble(start, count) {
  const lines = [];
  let addr = start;
  for (let i = 0; i < count; i++) {
    try {
      const insn = Instruction.parse(addr);
      lines.push(addr.toString() + ": " + insn.mnemonic + " " + insn.opStr);
      addr = insn.next;
    } catch (_e) {
      lines.push(addr.toString() + ": <无法解析>");
      addr = addr.add(1);
    }
  }
  return lines;
}

// 在驱动/框架模块内扫描 READ_CHARACTERISTIC_IOCTL 常量（0x80018483 小端），
// 定位调用读特征 IOCTL 的代码位置。
function scanIoCtrlCode() {
  const names = [
    "microsoft.bluetooth.profiles.hidovergatt.dll",
    "WUDFx02000.dll",
  ];
  const results = [];
  for (const name of names) {
    const mod = Process.findModuleByName(name);
    if (!mod) {
      continue;
    }
    try {
      const matches = Memory.scanSync(mod.base, mod.size, "83 84 01 80");
      for (const m of matches) {
        results.push(name + "+" + m.address.sub(mod.base).toString());
      }
    } catch (_e) {}
  }
  return results;
}

// 在调用栈里找驱动模块（hidovergatt.dll）的返回地址。
function findDriverFrame(context) {
  const driver = Process.findModuleByName(
    "microsoft.bluetooth.profiles.hidovergatt.dll"
  );
  if (!driver) {
    return null;
  }
  try {
    const bt = Thread.backtrace(context, Backtracer.ACCURATE);
    for (const addr of bt) {
      if (
        addr.compare(driver.base) >= 0 &&
        addr.compare(driver.base.add(driver.size)) < 0
      ) {
        return addr;
      }
    }
  } catch (_e) {}
  return null;
}

// 队列项（8 字节 = 2 字节头 + 3 个 16 位小端 usage）转成 9 字节 IOCTL
// 报告格式（01 00 00 + 3 个 usage），供应用侧 hogp_ioctl_payload 解析。
// 实测：上键队列项 00 00 52 00 00 00 00 00 ↔ IOCTL 01 00 00 52 00 00 00 00 00。
function queueToIoctlHex(src, len) {
  if (len !== 8) {
    return "";
  }
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

// 轮询设置页写入的 eat-mode.txt（内容为 "1"/"0"），运行中热切换吃掉模式。
function pollEatModeFile() {
  try {
    if (typeof File === "undefined") {
      return;
    }
    const file = new File(eatModePath, "r");
    const text = file.readText();
    file.close();
    const trimmed = text.trim();
    if (trimmed !== "1" && trimmed !== "0") {
      return;
    }
    const enabled = trimmed === "1";
    if (enabled !== eatEnabled) {
      eatEnabled = enabled;
      emit({ kind: "eat_mode_changed", eat_enabled: enabled });
    }
  } catch (_e) {}
}

function scheduleReconnect() {
  if (reconnectTimer !== null) {
    return;
  }
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connectToHub();
  }, RECONNECT_MS);
}

function markDisconnected(current) {
  if (output !== current) {
    return;
  }
  output = null;
  connection = null;
  scheduleReconnect();
}

function emit(payload) {
  const current = output;
  if (current === null) {
    scheduleReconnect();
    return;
  }
  const line = JSON.stringify(payload) + "\n";
  writeChain = writeChain
    .then(() => current.writeAll(asciiBytes(line)))
    .catch(() => markDisconnected(current));
}

async function connectToHub() {
  if (output !== null || connecting) {
    return;
  }
  connecting = true;
  try {
    const conn = await Socket.connect({
      family: "ipv4",
      host: host,
      port: port,
    });
    connection = conn;
    output = conn.output;
    emit({
      kind: "ready",
      pid: Process.id,
      hook_installed: hookInstalled,
      eat_enabled: eatEnabled,
      trace_enabled: traceEnabled,
    });
    if (traceEnabled) {
      emit({ kind: "module_list", modules: enumerateHidModules() });
      emit({ kind: "import_list", imports: enumerateImports() });
      emit({ kind: "scan_result", addresses: scanIoCtrlCode() });
      if (!globalDumped) {
        globalDumped = true;
        dumpGlobalTable();
      }
    }
  } catch (_error) {
    output = null;
    connection = null;
    scheduleReconnect();
  } finally {
    connecting = false;
  }
}

function installHook() {
  if (hookInstalled) {
    return;
  }
  const ntdll = Process.findModuleByName("ntdll.dll");
  const target = ntdll ? ntdll.findExportByName("NtDeviceIoControlFile") : null;
  if (target === null) {
    emit({ kind: "error", message: "NtDeviceIoControlFile export not found" });
    return;
  }
  Interceptor.attach(target, {
    onEnter(args) {
      stats.calls++;
      if (traceEnabled) {
        this.ioctl = args[5].toUInt32();
        this.input = args[6];
        this.inputLength = args[7].toUInt32();
        this.output = args[8];
        this.outputLength = args[9].toUInt32();
      }
      // 只对目标 IOCTL 记录输出缓冲区，避免触碰其它 IOCTL 的 args[8]。
      this.capture = args[5].toUInt32() === READ_CHARACTERISTIC_IOCTL;
      if (this.capture) {
        this.output = args[8];
        this.outputLength = args[9].toUInt32();
        if (traceEnabled) {
          // 返回地址是 kernel32/ntdll 的 DeviceIoControl 包装，不是驱动代码；
          // 从调用栈里找驱动模块的帧，反汇编驱动真正处理报告的位置。
          this.driverRet = findDriverFrame(this.context);
        }
      }
    },
    onLeave(retval) {
      if (traceEnabled && !this.capture) {
        // 追踪所有非目标 IOCTL（含零长度缓冲区）：记录 ioctl 码与状态，
        // 用于定位「报告从哪个 IOCTL/路径转发给 HID 类驱动」。
        try {
          const status = retval.toUInt32();
          emit({
            kind: "ioctl_trace",
            ioctl: this.ioctl,
            status: status,
            input_len: this.inputLength,
            output_len: this.outputLength,
            input_hex: hex(this.input, Math.min(this.inputLength, 32)),
            output_hex: hex(this.output, Math.min(this.outputLength, 32)),
          });
        } catch (_e) {}
      }
      if (!this.capture) {
        return;
      }
      stats.captured++;
      const status = retval.toUInt32();
      if (status === STATUS_SUCCESS) {
        stats.success++;
        if (this.outputLength === EXPECTED_OUTPUT_LENGTH && !this.output.isNull()) {
          try {
            const rawHex = hex(this.output, this.outputLength);
            if (rawHex) {
              // 吃掉模式下，应用调度信号改由 0x20080 转发点上报（那里
              // 才能拿到清零前的真实报告）；这里只上报原始读特征数据，
              // 避免同一按键被上报两次。
              if (!eatEnabled) {
                emit({ kind: "gatt_read", raw: rawHex, eaten: false });
              }
              if (traceEnabled && this.driverRet) {
                // 完整调用栈：看清驱动侧谁在发这个读特征 IOCTL。
                emit({
                  kind: "backtrace",
                  address: this.driverRet.toString(),
                  frames: fullBacktrace(this.context),
                });
              }
            }
          } catch (error) {
            emit({ kind: "error", message: String(error) });
          }
        }
      } else if (status === STATUS_PENDING) {
        // 只计数不读取：异步完成的缓冲区在 onLeave 时尚未填充，
        // 延迟读取可能读到已释放/复用的内存，反而更危险。
        stats.pending++;
      } else {
        stats.other_status++;
      }
    },
  });
  hookInstalled = true;
}

setInterval(() => {
  if (output === null) {
    scheduleReconnect();
  } else {
    emit({ kind: "heartbeat", pid: Process.id, stats: stats });
  }
}, HEARTBEAT_MS);

setInterval(() => {
  pollEatModeFile();
}, EAT_POLL_MS);

rpc.exports = {
  async init(_stage, parameters) {
    if (parameters && parameters.host) {
      host = parameters.host;
    }
    if (parameters && parameters.port) {
      port = parameters.port;
    }
    if (parameters && parameters.eat_file) {
      eatModePath = parameters.eat_file;
    }
    if (parameters && parameters.eat !== undefined) {
      eatEnabled = parameters.eat === true || parameters.eat === "true";
    }
    if (parameters && parameters.trace !== undefined) {
      traceEnabled = parameters.trace === true || parameters.trace === "true";
    }
    // 0x20080 转发点钩子必须始终安装：eat 可运行时热切换，
    // 不能在初始化时因 eat=false 而漏装。
    installReportPathHooks();
    if (traceEnabled) {
      installWriteFileTrace();
    }
    installHook();
    await connectToHub();
  },
};

try {
  // 只装 hook，不在这里连接：连接由 rpc.exports.init 在读取 config 参数
  // （host/port/eat）之后发起，避免 ready 消息带上默认的 eatEnabled=false。
  installHook();
} catch (_error) {}
