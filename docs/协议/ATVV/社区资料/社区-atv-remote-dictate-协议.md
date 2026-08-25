# ATVV protocol notes (Android TV voice remote over BLE)

_Worked out by probing a Google "Chromecast Remote"; later found to match Google's publicly-available [Voice over BLE spec v1.0](https://wangefan.github.io/linux_kernel_driver/resources/Google_Voice_over_BLE_spec_v1.0.pdf). Originally written up for [bluez/bluez#1086](https://github.com/bluez/bluez/issues/1086)._

---

# Google TV / Android TV remote microphone: working protocol notes (ATVV over GATT)

I got the built-in mic working on Linux with a **Google "Chromecast Remote"** (`Modalias bluetooth:v18D1p9450d0110`) — capturing audio and running it through speech-to-text. Everything below was reverse-engineered by black-box probing against that one remote; no vendor documentation was involved.

The OP's Ugoos UR02 exposes the same vendor service (`ab5e0001-5a21-4f05-bc7d-af01f617b664`), so this should transfer, but I can only *confirm* it for the Google remote. Posting in case it is useful to anyone implementing this properly.

Summary of the useful bits:

1. Pairing these remotes needs an agent that can answer a numeric-comparison prompt.
2. Audio is **not** carried over HID — it's the vendor GATT service (ATVV).
3. `MIC_OPEN` must be **exactly 2 bytes** and must be sent **while the remote is in search state**. Getting either wrong produces total silence with no error, which I suspect is what has stalled previous attempts.
4. Audio is plain **4-bit IMA/DVI ADPCM, 16 kHz mono**, decodable with stdlib `audioop`.

---

## 1. Pairing: `AuthenticationFailed` is a red herring

Every `bluetoothctl pair` attempt failed with `org.bluez.Error.AuthenticationFailed`. `btmon` shows why:

```
SMP: Pairing Request (0x01)
    IO capability: KeyboardDisplay (0x04)
    Authentication requirement: No bonding, MITM, SC, No Keypresses, CT2 (0x2c)
SMP: Pairing Response (0x02)
    IO capability: NoInputNoOutput (0x03)
    Authentication requirement: Bonding, No MITM, SC, No Keypresses (0x09)
...
@ MGMT Event: User Confirmation Request
= bluetoothd: device_confirm_passkey: Operation not permitted
@ MGMT Command: User Confirmation Negative Reply
SMP: Pairing Failed (0x05) Reason: Numeric comparison failed (0x0c)
```

The remote requests **MITM + LE Secure Connections**, i.e. numeric comparison. With a `NoInputNoOutput` agent, bluetoothd cannot answer the confirmation and sends a **negative reply**, so the bond is rejected. It is not Fast Pair (the remote does not expose `0xFE2C`) and not a lock to its old host.

**Fix:** pair with an agent that confirms the comparison — e.g. `agent DisplayYesNo` in `bluetoothctl` and answer `yes` at the prompt (the remote has no display, so it is a blind accept). After that: `Paired: yes`, `Bonded: yes`, and it reconnects on keypress.

Once bonded it also shows up as a normal input device (`/dev/input/eventN`, "Chromecast Remote"), so the buttons work as consumer keys.

## 2. Audio is not on HID

Worth ruling out explicitly: the HID report descriptor is 57 bytes, consumer page only, no vendor collection and no audio report:

```
05 0c 09 01 a1 01 85 01 0a 9e 01 09 cd 09 42 09 43 09 44 09 45 09 41 09 e2
0a 21 02 0a 23 02 0a 24 02 09 e9 09 ea 09 77 09 78 09 79 09 89 15 01 25 11
95 02 75 08 81 00 c0
```

So the "ADPCM chunked in HID reports" approach used by some older Android TV remotes does not apply here.

## 3. The ATVV GATT service

Service `ab5e0001-5a21-4f05-bc7d-af01f617b664` with three characteristics:

| UUID | Direction | Role |
|---|---|---|
| `ab5e0002-…` | write (no response) | **TX** — host → remote commands |
| `ab5e0003-…` | notify | **RX** — audio frames |
| `ab5e0004-…` | notify | **CTL** — control messages |

Opcodes observed (names follow Nordic's `ble_atvv` naming, which matches the behaviour):

Host → TX: `GET_CAPS = 0x0A`, `MIC_OPEN = 0x0C`, `MIC_CLOSE = 0x0D`
Remote → CTL: `0x00 AUDIO_STOP`, `0x04 AUDIO_START`, `0x08 START_SEARCH`, `0x0A AUDIO_SYNC`, `0x0B CAPS_RESP`, `0x0C MIC_OPEN_ERROR`

### Working exchange

```
        -> TX  0a 01 00                       GET_CAPS (version 1.0)
CTL     <- 0b 01 00 02 00 00 a0 01 00 1a      CAPS_RESP
   (user presses the assistant/mic button)
CTL     <- 08                                 START_SEARCH
        -> TX  0c 02                          MIC_OPEN     <-- exactly 2 bytes
CTL     <- 04 00 02 00                        AUDIO_START
RX      <- 160-byte frames @ 50/s             audio
        -> TX  0d 00                          MIC_CLOSE
CTL     <- 00 00                              AUDIO_STOP
```

`CAPS_RESP` interpretation is partly guesswork; what is certain is that `02 00` matches the codec field echoed in `AUDIO_START` (`04 00 02 00`), and `00 a0` = 160 matches the observed frame size.

### The three traps

**(a) `MIC_OPEN` must be exactly 2 bytes.** `0x0C` + one byte. Every longer payload I tried
(`0c 02 00`, `0c 00 02`, `0c 01 00 02 00`, `0c 01 00 00 02`) was **silently discarded** — no audio, no error, no CTL message at all. A bare `0c` is likewise ignored. The payload byte itself appears not to matter: `00, 01, 02, 03, 04, 08, 0f` all produced `AUDIO_START`.

**(b) `MIC_OPEN` only works in search state.** Sent at any other time it returns `MIC_OPEN_ERROR`:

```
-> 0c 00     <- CTL  0c 0f 02
```

So the host cannot switch the mic on unilaterally; the user must press the assistant button first, and the host answers the resulting `START_SEARCH`. (Receiving `0c 0f 02` is actually good news when debugging — it means the command parsed.)

**(c) `MIC_CLOSE` is `0d 00`,** and unlike `MIC_OPEN` the payload byte *does* matter: `0d 02` left the stream running (125 frames in the next 2.5 s), while `0d 00` produced `AUDIO_STOP` and the frames ceased. If you never close the session, the remote keeps streaming indefinitely (I saw >50 s) and will not emit a new `START_SEARCH` until it is closed — so a client that exits without closing leaves the mic wedged open.

## 4. Audio format

RX notifications are **160 bytes each at 50/s = 8000 bytes/s**, which is exactly 4-bit ADPCM at 16 kHz (320 samples = 20 ms per frame). There is **no per-frame header and no per-frame state reset** — it is one continuous IMA/DVI ADPCM stream, so stdlib `audioop` decodes it directly:

```python
import audioop, wave
pcm, _ = audioop.adpcm2lin(raw_adpcm, 2, None)      # -> s16le
with wave.open("out.wav", "wb") as w:
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
    w.writeframes(pcm)
```

(Note `audioop` was removed in Python 3.13; use `audioop-lts` or any IMA decoder.)

Result is clean speech — I ran it straight into faster-whisper and got exact transcriptions, so the decode is correct, not merely intelligible.

## 5. Minimal working example

One caveat for anyone experimenting: **scan-based libraries such as bleak do not work here.** The remote stops advertising when idle, so connect-by-address fails with "device not found" even while BlueZ holds a perfectly good connection. Going through BlueZ's D-Bus API works.

This is the exact script I ran (needs `pip install dbus-fast`; the device must already be paired and trusted):

```python
#!/usr/bin/env python3
"""Minimal, self-contained demo: capture audio from an Android TV / Google TV
BLE remote's built-in microphone on Linux, via the ATVV GATT service.

Usage:  python3 atvv_mic.py <MAC>            # e.g. AA:BB:CC:DD:EE:FF
        (device must already be paired+trusted; press the assistant/mic
         button when prompted)

Writes out.wav (16 kHz mono) in the current directory.

Requires: python3 -m pip install dbus-fast     (plus stdlib audioop, so
          Python <= 3.12; on 3.13+ use audioop-lts or your own IMA decoder)

Why D-Bus and not bleak/GATTlib: these remotes stop advertising when idle, so
connect-by-address in scan-based libraries fails with "device not found". Going
through BlueZ's D-Bus API uses the link BlueZ already maintains.
"""
import asyncio
import audioop
import sys
import time
import wave

from dbus_fast import BusType, Variant
from dbus_fast.aio import MessageBus

ATVV_TX  = "ab5e0002-5a21-4f05-bc7d-af01f617b664"  # write  (host -> remote)
ATVV_RX  = "ab5e0003-5a21-4f05-bc7d-af01f617b664"  # notify (audio)
ATVV_CTL = "ab5e0004-5a21-4f05-bc7d-af01f617b664"  # notify (control)

GET_CAPS  = bytes([0x0A, 0x01, 0x00])
MIC_OPEN  = bytes([0x0C, 0x02])   # MUST be exactly 2 bytes
MIC_CLOSE = bytes([0x0D, 0x00])

CTL = {0x00: "AUDIO_STOP", 0x04: "AUDIO_START", 0x08: "START_SEARCH",
       0x0A: "AUDIO_SYNC", 0x0B: "CAPS_RESP", 0x0C: "MIC_OPEN_ERROR"}

RECORD_SECS = 8


async def main(mac):
    devp = "/org/bluez/hci0/dev_" + mac.upper().replace(":", "_")
    bus = await MessageBus(bus_type=BusType.SYSTEM).connect()

    async def iface(path, name):
        intro = await bus.introspect("org.bluez", path)
        obj = bus.get_proxy_object("org.bluez", path, intro)
        return obj, obj.get_interface(name)

    dobj, dev = await iface(devp, "org.bluez.Device1")
    dprops = dobj.get_interface("org.freedesktop.DBus.Properties")
    try:
        await dev.call_connect()
    except Exception as e:
        print(f"connect: {e} (press a button to wake the remote)")
    for _ in range(40):
        try:
            if (await dprops.call_get("org.bluez.Device1", "ServicesResolved")).value:
                break
        except Exception:
            pass
        await asyncio.sleep(0.5)
    else:
        sys.exit("not connected / services never resolved")

    # discover the three ATVV characteristics by UUID (paths differ per device)
    omobj = bus.get_proxy_object("org.bluez", "/",
                                 await bus.introspect("org.bluez", "/"))
    om = omobj.get_interface("org.freedesktop.DBus.ObjectManager")
    found = {}
    for path, ifaces in (await om.call_get_managed_objects()).items():
        c = ifaces.get("org.bluez.GattCharacteristic1")
        if c and path.startswith(devp):
            found[c["UUID"].value.lower()] = path
    try:
        txp, rxp, ctlp = found[ATVV_TX], found[ATVV_RX], found[ATVV_CTL]
    except KeyError:
        sys.exit(f"ATVV characteristics not found; device exposes: "
                 f"{sorted(found)}")
    print(f"TX  {txp}\nRX  {rxp}\nCTL {ctlp}")

    _, txc = await iface(txp, "org.bluez.GattCharacteristic1")
    rxo, rxc = await iface(rxp, "org.bluez.GattCharacteristic1")
    ctlo, ctlc = await iface(ctlp, "org.bluez.GattCharacteristic1")

    adpcm = bytearray()
    state = {"recording": False, "frames": 0, "stopped_at": None}
    t0 = time.monotonic()

    async def send(name, data):
        await txc.call_write_value(data, {"type": Variant("s", "command")})
        print(f"[{time.monotonic()-t0:6.2f}] --> {name} {data.hex(' ')}")

    def on_rx(_i, changed, _inv):
        if "Value" in changed and state["recording"]:
            adpcm.extend(bytes(changed["Value"].value))
            state["frames"] += 1
        elif "Value" in changed and state["stopped_at"]:
            # frames arriving after MIC_CLOSE -- report them
            state["frames"] += 1

    def on_ctl(_i, changed, _inv):
        if "Value" not in changed:
            return
        v = bytes(changed["Value"].value)
        print(f"[{time.monotonic()-t0:6.2f}] CTL {CTL.get(v[0], hex(v[0]))}: {v.hex(' ')}")
        if v[0] == 0x08:                      # START_SEARCH
            asyncio.create_task(send("MIC_OPEN", MIC_OPEN))
        elif v[0] == 0x04:                    # AUDIO_START
            state["recording"] = True

    rxo.get_interface("org.freedesktop.DBus.Properties").on_properties_changed(on_rx)
    ctlo.get_interface("org.freedesktop.DBus.Properties").on_properties_changed(on_ctl)
    await rxc.call_start_notify()
    await ctlc.call_start_notify()
    await send("GET_CAPS", GET_CAPS)

    print(">>> press the ASSISTANT/MIC button and speak")
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        await asyncio.sleep(0.2)
        if state["recording"] and len(adpcm) >= RECORD_SECS * 8000:
            break
    if not adpcm:
        sys.exit("no audio received")

    # verify MIC_CLOSE actually stops the stream
    state["recording"] = False
    before = state["frames"]
    await send("MIC_CLOSE", MIC_CLOSE)
    state["stopped_at"] = time.monotonic()
    await asyncio.sleep(3)
    after = state["frames"]
    print(f"frames after MIC_CLOSE: {after - before} (0 == stream stopped)")

    pcm, _ = audioop.adpcm2lin(bytes(adpcm), 2, None)
    with wave.open("out.wav", "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(pcm)
    print(f"wrote out.wav: {len(adpcm)} ADPCM bytes -> {len(pcm)/2/16000:.1f}s, "
          f"rms={audioop.rms(pcm, 2)}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    asyncio.run(main(sys.argv[1]))
```

Output on my remote:

```
TX  /org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF/service0028/char0029
RX  /org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF/service0028/char002b
CTL /org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF/service0028/char002e
[  0.15] --> GET_CAPS 0a 01 00
>>> press the ASSISTANT/MIC button and speak
[  0.19] CTL CAPS_RESP: 0b 01 00 02 00 00 a0 01 00 1a
[  0.52] CTL START_SEARCH: 08
[  0.52] --> MIC_OPEN 0c 02
[  0.57] CTL AUDIO_START: 04 00 02 00
[  8.78] --> MIC_CLOSE 0d 00
[  8.83] CTL AUDIO_STOP: 00 00
frames after MIC_CLOSE: 2 (0 == stream stopped)
wrote out.wav: 64480 ADPCM bytes -> 8.1s, rms=664
```

## What I have not worked out

- The meaning of the `MIC_OPEN` payload byte (any value works), and of the trailing `01 00 1a` in `CAPS_RESP`.
- The `MIC_OPEN_ERROR` payload `0f 02`.
- `AUDIO_SYNC (0x0A)` — never observed in my captures.
- Whether other codecs can be negotiated; this remote only ever advertised/used `0x0002`.
- Whether any of this differs on non-Google remotes using the same service.

Happy to run further experiments against this remote if it would help someone implementing kernel or BlueZ-side support.
