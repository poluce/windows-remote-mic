// 只读的 HOGP 报告探针。不修改缓冲区，因此键盘 Raw Input 保持完整。
const READ_CHARACTERISTIC_IOCTL = 0x80018483;
const EXPECTED_OUTPUT_LENGTH = 9;
const HEARTBEAT_MS = 5000;
const RECONNECT_MS = 1000;

let host = "127.0.0.1";
let port = 17331;
let output = null;
let writeChain = Promise.resolve();
let reconnectTimer = null;
let hookInstalled = false;

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
  if (output !== null) {
    return;
  }
  try {
    const connection = await Socket.connect({
      family: "ipv4",
      host: host,
      port: port,
    });
    output = connection.output;
    emit({ kind: "ready", pid: Process.id, hook_installed: hookInstalled });
  } catch (_error) {
    output = null;
    scheduleReconnect();
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
      this.ioctl = args[5].toUInt32();
      this.output = args[8];
      this.outputLength = args[9].toUInt32();
      // 捕获所有输出缓冲区长度 >= 3 的 IOCTL 调用
      this.capture = this.outputLength >= 3 && !this.output.isNull();
    },
    onLeave(retval) {
      if (!this.capture || this.output.isNull()) {
        return;
      }
      // 严格仅在 STATUS_SUCCESS (0) 时读取数据；绝不能在 STATUS_PENDING (0x103) 时读取未完成的内核缓冲区
      if (retval.toUInt32() !== 0) {
        return;
      }
      try {
        const rawHex = hex(this.output, Math.min(this.outputLength, 64));
        if (!rawHex) {
          return;
        }
        if (this.ioctl === READ_CHARACTERISTIC_IOCTL || rawHex.startsWith("010000")) {
          emit({
            kind: "gatt_read",
            raw: rawHex,
          });
        }
      } catch (error) {
        emit({ kind: "error", message: String(error) });
      }
    },
  });
  hookInstalled = true;
}

setInterval(() => {
  if (output === null) {
    scheduleReconnect();
  } else {
    emit({ kind: "heartbeat", pid: Process.id });
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
    installHook();
    await connectToHub();
  },
};

try {
  installHook();
  connectToHub();
} catch (_error) {}
