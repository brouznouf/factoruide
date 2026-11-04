// A Web Worker running the backend (crates/fg-web, built for wasm32-wasip1-threads): the main
// one answers the page's calls, the others are its threads, each started by `thread-spawn`.
// All share one WebAssembly memory. The page (host.ts) starts them and relays between them.
//
// The backend needs little of WASI: clocks, randomness, sleeping, and stdout/stderr for its
// panics; no files (the page keeps them).

/// <reference lib="webworker" />

export type Init = {
  module: WebAssembly.Module;
  memory: WebAssembly.Memory;
  /** Next thread id, shared by every worker (Int32Array over a SharedArrayBuffer). */
  tids: SharedArrayBuffer;
  /** The page's address: relative URLs are resolved from it. */
  base: string;
} & ({ role: "main"; files: [string, ArrayBuffer][] } | { role: "thread"; tid: number; arg: number });

export type FromWorker =
  | { ready: true }
  | { reply: number; json: string }
  | { event: string }
  | { persist: string; data: ArrayBuffer | null }
  | { spawn: number; arg: number }
  | { log: string; error: boolean }
  | { crash: string };

declare const self: DedicatedWorkerGlobalScope;
const post = (m: FromWorker, transfer: Transferable[] = []) => self.postMessage(m, transfer);

// WASI errors used.
const SUCCESS = 0;

/** The backend's environment: room for the planner's threads, whose stacks hold more in
 *  WebAssembly than natively. */
const ENV = ["RUST_MIN_STACK=16777216"].map((e) => new TextEncoder().encode(e));
const EBADF = 8;
const EINVAL = 28;
const ENOSYS = 52;

let memory: WebAssembly.Memory;
let exports: {
  fg_alloc(len: number): number;
  fg_load(path: number, pathLen: number, data: number, dataLen: number): void;
  fg_call(id: number, request: number, len: number): void;
  wasi_thread_start(tid: number, arg: number): void;
  /** The libc's start (a reactor module's `_initialize`). */
  ["_initialize"](): void;
  fg_reserve(max: number): number;
};

const bytes = () => new Uint8Array(memory.buffer);
const view = () => new DataView(memory.buffer);
/** The functions the module imports, their pointers and sizes read unsigned (past 2 GiB, wasm's
 *  i32 arrive negative). */
function unsigned<T extends Record<string, (...a: never[]) => unknown>>(fns: T): T {
  return Object.fromEntries(
    Object.entries(fns).map(([name, f]) => [name, (...args: unknown[]) => (f as (...a: unknown[]) => unknown)(...args.map((a) => (typeof a === "number" ? a >>> 0 : a)))]),
  ) as unknown as T;
}

/** A copy out of the shared memory (TextDecoder and XHR refuse shared buffers). */
/** Pointers and lengths come as signed 32-bit integers: past 2 GiB they read negative. */
const u32 = (n: number) => n >>> 0;
const copyOut = (ptr: number, len: number) => bytes().slice(u32(ptr), u32(ptr) + u32(len));
const text = (ptr: number, len: number) => new TextDecoder().decode(copyOut(ptr, len));

/** A buffer of the backend's (`fg_alloc`) holding `data`, handed over to it. */
function copyIn(data: Uint8Array): number {
  const ptr = u32(exports.fg_alloc(data.length));
  bytes().set(data, ptr);
  return ptr;
}

// What the backend writes on stdout and stderr, line by line.
const lines: Record<number, string> = { 1: "", 2: "" };
function write(fd: number, chunk: string) {
  const all = lines[fd] + chunk;
  const parts = all.split("\n");
  lines[fd] = parts.pop() ?? "";
  for (const line of parts) post({ log: line, error: fd === 2 });
}

/** A shared word to wait on for sleeping (Atomics.wait is allowed in workers). */
const sleeper = new Int32Array(new SharedArrayBuffer(4));
const sleep = (ms: number) => {
  if (ms > 0) Atomics.wait(sleeper, 0, 0, ms);
};

function wasi(): Record<string, WebAssembly.ImportValue> {
  const impl: Record<string, (...a: never[]) => number | void> = {
    args_sizes_get(argc: number, size: number) {
      view().setUint32(argc, 0, true);
      view().setUint32(size, 0, true);
      return SUCCESS;
    },
    args_get: () => SUCCESS,
    environ_sizes_get(count: number, size: number) {
      view().setUint32(count, ENV.length, true);
      view().setUint32(size, ENV.reduce((n, e) => n + e.length + 1, 0), true);
      return SUCCESS;
    },
    environ_get(ptrs: number, buf: number) {
      let at = buf;
      ENV.forEach((e, i) => {
        view().setUint32(ptrs + 4 * i, at, true);
        bytes().set(e, at);
        bytes()[at + e.length] = 0;
        at += e.length + 1;
      });
      return SUCCESS;
    },
    clock_res_get(_id: number, out: number) {
      view().setBigUint64(out, 1000n, true);
      return SUCCESS;
    },
    clock_time_get(id: number, _precision: bigint, out: number) {
      const ns =
        id === 0
          ? BigInt(Date.now()) * 1_000_000n
          : BigInt(Math.round((performance.timeOrigin + performance.now()) * 1_000_000));
      view().setBigUint64(out, ns, true);
      return SUCCESS;
    },
    random_get(buf: number, len: number) {
      for (let at = 0; at < len; at += 65536) {
        const part = new Uint8Array(Math.min(65536, len - at));
        crypto.getRandomValues(part);
        bytes().set(part, buf + at);
      }
      return SUCCESS;
    },
    fd_write(fd: number, iovs: number, count: number, written: number) {
      if (fd !== 1 && fd !== 2) return EBADF;
      let total = 0;
      let out = "";
      for (let i = 0; i < count; i++) {
        const ptr = view().getUint32(iovs + i * 8, true);
        const len = view().getUint32(iovs + i * 8 + 4, true);
        out += text(ptr, len);
        total += len;
      }
      write(fd, out);
      view().setUint32(written, total, true);
      return SUCCESS;
    },
    fd_fdstat_get(fd: number, out: number) {
      if (fd > 2) return EBADF;
      const v = view();
      v.setUint8(out, 2); // character device
      v.setUint16(out + 2, 0, true);
      v.setBigUint64(out + 8, 0n, true);
      v.setBigUint64(out + 16, 0n, true);
      return SUCCESS;
    },
    fd_prestat_get: () => EBADF,
    fd_close: () => EBADF,
    proc_exit(code: number) {
      throw new Error(`backend exited (${code})`);
    },
    sched_yield: () => SUCCESS,
    /** Only clock subscriptions (sleeps): waits for the earliest. */
    poll_oneoff(subs: number, events: number, count: number, done: number) {
      const v = view();
      let wait = Infinity;
      let first = -1;
      for (let i = 0; i < count; i++) {
        const s = subs + i * 48;
        if (v.getUint8(s + 8) !== 0) return ENOSYS;
        const clock = v.getUint32(s + 16, true);
        const timeout = Number(v.getBigUint64(s + 24, true)) / 1e6;
        const absolute = (v.getUint16(s + 40, true) & 1) === 1;
        const now = clock === 0 ? Date.now() : performance.timeOrigin + performance.now();
        const ms = absolute ? timeout - now : timeout;
        if (ms < wait) {
          wait = ms;
          first = i;
        }
      }
      if (first < 0) return EINVAL;
      sleep(wait);
      const s = subs + first * 48;
      const e = events;
      v.setBigUint64(e, v.getBigUint64(s, true), true);
      v.setUint16(e + 8, 0, true);
      v.setUint8(e + 10, 0);
      view().setUint32(done, 1, true);
      return SUCCESS;
    },
  };
  const all = new Proxy(unsigned(impl), {
    get: (target, name: string) => target[name] ?? (() => (name.startsWith("fd_") ? EBADF : ENOSYS)),
  });
  return Object.fromEntries(
    WebAssembly.Module.imports(currentModule)
      .filter((i) => i.module === "wasi_snapshot_preview1")
      .map((i) => [i.name, all[i.name]]),
  );
}

let currentModule: WebAssembly.Module;

function fg(base: string): Record<string, WebAssembly.ImportValue> {
  return unsigned({
    fg_reply(id: number, ptr: number, len: number) {
      post({ reply: id, json: text(ptr, len) });
    },
    fg_event(ptr: number, len: number) {
      post({ event: text(ptr, len) });
    },
    fg_persist(op: number, path: number, pathLen: number, data: number, dataLen: number) {
      const copy = op === 0 ? copyOut(data, dataLen).buffer : null;
      post({ persist: text(path, pathLen), data: copy }, copy ? [copy] : []);
    },
    /** A synchronous request (allowed in workers): the backend waits for its answer anyway. */
    fg_fetch(request: number, requestLen: number, body: number, bodyLen: number, out: number) {
      const req = JSON.parse(text(request, requestLen)) as { method: string; url: string; content_type: string | null };
      const answer = (status: number, data: Uint8Array, meta: object) => {
        const m = new TextEncoder().encode(JSON.stringify(meta));
        const v = view();
        v.setUint32(out, data.length ? copyIn(data) : 0, true);
        v.setUint32(out + 4, data.length, true);
        v.setUint32(out + 8, copyIn(m), true);
        v.setUint32(out + 12, m.length, true);
        return status;
      };
      try {
        const xhr = new XMLHttpRequest();
        xhr.open(req.method, new URL(req.url, base).href, false);
        xhr.responseType = "arraybuffer";
        if (req.content_type) xhr.setRequestHeader("Content-Type", req.content_type);
        xhr.send(req.method === "GET" ? null : copyOut(body, bodyLen));
        const data = new Uint8Array((xhr.response as ArrayBuffer | null) ?? new ArrayBuffer(0));
        return answer(xhr.status, data, { retry_after: xhr.getResponseHeader("Retry-After") });
      } catch (e) {
        return answer(0, new Uint8Array(0), { error: String(e) });
      }
    },
  });
}

async function start(init: Init) {
  memory = init.memory;
  currentModule = init.module;
  const tids = new Int32Array(init.tids);
  const imports = {
    env: { memory },
    wasi_snapshot_preview1: wasi(),
    wasi: {
      "thread-spawn": (arg: number) => {
        const tid = Atomics.add(tids, 0, 1);
        post({ spawn: tid, arg });
        return tid;
      },
    },
    fg: fg(init.base),
  };
  const instance = await WebAssembly.instantiate(init.module, imports);
  exports = instance.exports as unknown as typeof exports;
  if (init.role === "thread") {
    exports.wasi_thread_start(init.tid, init.arg);
    // The thread is over: its worker too.
    self.close();
    return;
  }
  // Once, in the main worker (the threads share what it initialized). The memory grows now,
  // before any thread: a thread copying memory another one just grew traps (see fg_reserve).
  exports["_initialize"]();
  const reserved = u32(exports.fg_reserve(3.75 * 1024 ** 3));
  post({ log: `memory reserved: ${reserved >>> 20} MiB`, error: false });
  for (const [path, data] of init.files) {
    const p = new TextEncoder().encode(path);
    const d = new Uint8Array(data);
    exports.fg_load(copyIn(p), p.length, copyIn(d), d.length);
  }
  post({ ready: true });
}

self.onmessage = (e: MessageEvent<Init | { call: number; request: string }>) => {
  const m = e.data;
  try {
    if ("call" in m) {
      const req = new TextEncoder().encode(m.request);
      exports.fg_call(m.call, copyIn(req), req.length);
    } else {
      start(m).catch((err) => post({ crash: `${err}\n${err?.stack ?? ""} (memory size ${memory?.buffer.byteLength >>> 20} MiB, reserved at the start)` }));
    }
  } catch (err) {
    post({ crash: `${err}\n${(err as Error)?.stack ?? ""}` });
  }
};
