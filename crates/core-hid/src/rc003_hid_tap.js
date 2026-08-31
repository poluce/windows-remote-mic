// 只读的 HOGP 报告探针。不修改缓冲区，因此键盘 Raw Input 保持完整。
//
// 关键约束（与已验证可用的 remote-bridge-hub 实现一致）：
// - 只 hook 目标 IOCTL `READ_CHARACTERISTIC_IOCTL`(0x80018483)；
// - 只在返回 STATUS_SUCCESS 且输出长度恰为 9 字节时读取输出缓冲区。
// 之前放宽到「捕获所有带输出缓冲区的 IOCTL」后，onLeave 会对每个 IOCTL
// 都 readByteArray(args[8])，某些 IOCTL 的输出缓冲区不是普通可读内存
// （MDL / 内核指针 / 已释放缓冲），会把 QuickJS 事件循环卡死——表现为
// 第一次按键能上报，之后心跳与后续按键全部消失。

const READ_CHARACTERISTIC_IOCTL = 0x80018483;
const EXPECTED_OUTPUT_LENGTH = 9;
const STATUS_SUCCESS = 0;
const STATUS_PENDING = 0x103;
const HEARTBEAT_MS = 5000;
const RECONNECT_MS = 1000;

let host = "127.0.0.1";
let port = 17331;
// 保持 SocketConnection 引用，防止 QuickJS GC 提前关闭 socket。
let connection = null;
let output = null;
let connecting = false;
let reconnectTimer = null;
let hookInstalled = false;

// 串行写链：每次 writeAll 都排在上一次之后，避免并发写把 socket 写坏。
// 之前「队列 + 定时 flush」不等待上一次写完成，一旦某次写失败/卡住，
// 后续所有上报（心跳 + 按键）都会丢失——表现为只收到第一条报告。
let writeChain = Promise.resolve();

// hook 统计，随心跳上报，用于确认 hook 是否持续被调用。
let stats = {
  calls: 0,
  captured: 0,
  success: 0,
  pending: 0,
  other_status: 0,
  heartbeats: 0,
  writes_queued: 0,
  writes_done: 0,
  write_fails: 0,
};

// 诊断：把统计直接写文件，绕过 TCP/事件循环，用于判断 hook 是否仍在被调用。
const STATS_FILE = "C:\\ProgramData\\RemoteMic\\hid-tap\\hook-stats.txt";
function dumpStatsToFile() {
  try {
    File.writeAllText(
      STATS_FILE,
      JSON.stringify(stats) + " t=" + Date.now() + "\n"
    );
  } catch (_e) {}
}

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
  stats.writes_queued++;
  const line = JSON.stringify(payload) + "\n";
  writeChain = writeChain
    .then(() => current.writeAll(asciiBytes(line)))
    .then(() => {
      stats.writes_done++;
    })
    .catch(() => {
      stats.write_fails++;
      dumpStatsToFile();
      markDisconnected(current);
    });
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
    emit({ kind: "ready", pid: Process.id, hook_installed: hookInstalled });
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
      // 每 5 次调用写一次文件，确认 hook 是否持续被调用（绕过事件循环）。
      if (stats.calls % 5 === 0) {
        dumpStatsToFile();
      }
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
              emit({ kind: "gatt_read", raw: rawHex });
            }
          } catch (error) {
            emit({ kind: "error", message: String(error) });
          }
        }
        dumpStatsToFile();
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
  stats.heartbeats++;
  if (output === null) {
    scheduleReconnect();
  } else {
    emit({ kind: "heartbeat", pid: Process.id, stats: stats });
  }
  dumpStatsToFile();
}, HEARTBEAT_MS);

rpc.exports = {
  async init(_stage, parameters) {
    if (parameters && parameters.host) {
      host = parameters.host;
    }
    if (parameters && parameters.port) {
      port = parameters.port;
    }
    installHook();
    await connectToHub();
  },
};

try {
  installHook();
  connectToHub();
} catch (_error) {}
