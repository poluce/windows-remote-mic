// HOGP 报告探针。默认只读；开启「吃掉」模式后，会把整份报告的 usage 区
// 清零，让系统 HOGP 层看不到任何遥控器按键，只由本应用注入映射动作。
//
// 关键约束（与已验证可用的 remote-bridge-hub 实现一致）：
// - 只 hook 目标 IOCTL `READ_CHARACTERISTIC_IOCTL`(0x80018483)；
// - 只在返回 STATUS_SUCCESS 且输出长度恰为 9 字节时读取输出缓冲区。
// 不读取其它 IOCTL 的 args[8]：那可能是 MDL / 内核指针 / 已释放缓冲，
// 读它既无意义又有风险。

const READ_CHARACTERISTIC_IOCTL = 0x80018483;
const EXPECTED_OUTPUT_LENGTH = 9;
const STATUS_SUCCESS = 0;
const STATUS_PENDING = 0x103;
const HEARTBEAT_MS = 5000;
const RECONNECT_MS = 1000;

let host = "127.0.0.1";
let port = 17331;
// 是否开启「吃掉」模式：由 Gadget config 的 parameters.eat 传入。
let eatEnabled = false;
// 保持 SocketConnection 引用，防止 QuickJS GC 提前关闭 socket。
let connection = null;
let output = null;
let connecting = false;
let reconnectTimer = null;
let hookInstalled = false;

// 串行写链：每次 writeAll 都排在上一次之后，避免并发写把 socket 写坏。
let writeChain = Promise.resolve();

// hook 统计，随心跳上报，用于确认 hook 是否持续被调用。
let stats = { calls: 0, captured: 0, success: 0, pending: 0, other_status: 0 };

function asciiBytes(text) {
  const out = [];
  for (let i = 0; i < text.length; i++) {
    out.push(text.charCodeAt(i) & 0xff);
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

// 吃掉全部按键：把输出缓冲区里的整个 usage 区（字节 3–8）清零。
// 报告格式：01 00 00 + 3 个 16 位小端 usage（共 9 字节）。
// 只对确定格式动手，其它情况一律跳过，避免误伤驱动缓冲区。
// 返回 true 表示本次确实清零了 usage（供日志记录）。
function eatUsages(outputPtr, length) {
  if (!eatEnabled || length !== EXPECTED_OUTPUT_LENGTH) {
    return false;
  }
  const bytes = new Uint8Array(outputPtr.readByteArray(length));
  if (bytes[0] !== 0x01 || bytes[1] !== 0x00 || bytes[2] !== 0x00) {
    return false;
  }
  let changed = false;
  for (let i = 3; i < length; i++) {
    if (bytes[i] !== 0) {
      bytes[i] = 0;
      changed = true;
    }
  }
  if (changed) {
    outputPtr.writeByteArray(bytes.buffer);
  }
  return changed;
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
    });
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
      // 只对目标 IOCTL 记录输出缓冲区，避免触碰其它 IOCTL 的 args[8]。
      this.capture = args[5].toUInt32() === READ_CHARACTERISTIC_IOCTL;
      if (this.capture) {
        this.output = args[8];
        this.outputLength = args[9].toUInt32();
      }
    },
    onLeave(retval) {
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
              // 先吃掉（清零），再上报原始报告 + 吃掉标记，供日志验证。
              const eaten = eatUsages(this.output, this.outputLength);
              emit({ kind: "gatt_read", raw: rawHex, eaten: eaten });
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

rpc.exports = {
  async init(_stage, parameters) {
    if (parameters && parameters.host) {
      host = parameters.host;
    }
    if (parameters && parameters.port) {
      port = parameters.port;
    }
    if (parameters && parameters.eat !== undefined) {
      eatEnabled = parameters.eat === true || parameters.eat === "true";
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
