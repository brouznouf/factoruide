// The page's side of the backend (crates/fg-web in Web Workers, see worker.ts): starts it with
// the files saved in the browser (IndexedDB), passes it the calls, starts its threads, saves
// what it writes and hands its events to the listeners.

import type { FromWorker, Init } from "./worker";
import wasmUrl from "./fg_web.wasm?url";

type Pending = { resolve: (v: unknown) => void; reject: (e: unknown) => void };

const DB_NAME = "factoruide";
const STORE = "files";

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function savedFiles(db: IDBDatabase): Promise<[string, ArrayBuffer][]> {
  return new Promise((resolve, reject) => {
    const out: [string, ArrayBuffer][] = [];
    const req = db.transaction(STORE).objectStore(STORE).openCursor();
    req.onsuccess = () => {
      const c = req.result;
      if (!c) return resolve(out);
      out.push([String(c.key), c.value as ArrayBuffer]);
      c.continue();
    };
    req.onerror = () => reject(req.error);
  });
}

/** Wasm pages (64 KiB) of the shared memory: as much as the browser gives, up to 4 GiB. */
function sharedMemory(): WebAssembly.Memory {
  for (const maximum of [65536, 32768, 16384]) {
    try {
      return new WebAssembly.Memory({ initial: 1024, maximum, shared: true });
    } catch {
      // A smaller reservation.
    }
  }
  throw new Error("the browser refused the memory the app needs");
}

export type Listener = (payload: unknown) => void;

export class Backend {
  private main!: Worker;
  private nextId = 1;
  private pending = new Map<number, Pending>();
  private listeners = new Map<string, Set<Listener>>();
  private ready: Promise<void>;
  private crashed: string | null = null;
  private db!: IDBDatabase;
  /** Writes in order, one transaction each (the backend may write a file twice in a row). */
  private writes: Promise<void> = Promise.resolve();

  constructor() {
    this.ready = this.start();
  }

  private async start() {
    if (!self.crossOriginIsolated) {
      throw new Error("not_isolated");
    }
    const [module, db] = await Promise.all([WebAssembly.compileStreaming(fetch(wasmUrl)), openDb()]);
    this.db = db;
    const files = await savedFiles(db);
    const memory = sharedMemory();
    const tids = new SharedArrayBuffer(4);
    new Int32Array(tids)[0] = 1;
    const base = document.baseURI;
    const common = { module, memory, tids, base };
    const spawn = (init: Init) => {
      const w = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
      w.onmessage = (e: MessageEvent<FromWorker>) => this.receive(e.data, common);
      w.onerror = (e) => this.crash(e.message || "worker error");
      w.postMessage(init);
      return w;
    };
    this.spawnThread = (tid, arg) => spawn({ ...common, role: "thread", tid, arg });
    await new Promise<void>((resolve, reject) => {
      this.onReady = resolve;
      this.onCrash = reject;
      this.main = spawn({ ...common, role: "main", files });
    });
  }

  /** The last lines the backend wrote on its error output. */
  private errors: string[] = [];
  private onReady: () => void = () => {};
  private onCrash: (e: unknown) => void = () => {};
  private spawnThread: (tid: number, arg: number) => void = () => {};

  private receive(m: FromWorker, _common: unknown) {
    if ("ready" in m) {
      this.onReady();
    } else if ("reply" in m) {
      const p = this.pending.get(m.reply);
      this.pending.delete(m.reply);
      const answer = JSON.parse(m.json) as { ok?: unknown; err?: string };
      if (answer.err !== undefined) p?.reject(answer.err);
      else p?.resolve(answer.ok ?? null);
    } else if ("event" in m) {
      const { event, payload } = JSON.parse(m.event) as { event: string; payload: unknown };
      for (const l of this.listeners.get(event) ?? []) l(payload);
    } else if ("persist" in m) {
      this.persist(m.persist, m.data);
    } else if ("spawn" in m) {
      this.spawnThread(m.spawn, m.arg);
    } else if ("log" in m) {
      (m.error ? console.error : console.log)(`[fg] ${m.log}`);
      // What the backend wrote on its error output (a panic says where and why): kept for a crash.
      if (m.error) this.errors = [...this.errors, m.log].slice(-8);
    } else if ("crash" in m) {
      this.crash(m.crash);
    }
  }

  private persist(path: string, data: ArrayBuffer | null) {
    this.writes = this.writes.then(
      () =>
        new Promise<void>((resolve) => {
          const tx = this.db.transaction(STORE, "readwrite");
          const store = tx.objectStore(STORE);
          if (data) store.put(data, path);
          else store.delete(path);
          tx.oncomplete = () => resolve();
          tx.onerror = () => {
            console.error(`could not save ${path}`, tx.error);
            resolve();
          };
        }),
    );
  }

  /** The backend stopped (a panic aborts it): every call under way fails, and the next ones. */
  private crash(trap: string) {
    // The panic's own message first (the stack of a trap does not carry it, in Firefox).
    const message = this.errors.length ? `${this.errors.join("\n")}\n${trap}` : trap;
    console.error(`[fg] backend stopped: ${message}`);
    this.crashed = message;
    this.onCrash(new Error(message));
    for (const p of this.pending.values()) p.reject(`the app stopped: ${message}`);
    this.pending.clear();
  }

  async call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    await this.ready;
    if (this.crashed) throw `the app stopped: ${this.crashed}`;
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.main.postMessage({ call: id, request: JSON.stringify({ cmd, args }) });
    });
  }

  listen(event: string, fn: Listener): () => void {
    let set = this.listeners.get(event);
    if (!set) this.listeners.set(event, (set = new Set()));
    set.add(fn);
    return () => set.delete(fn);
  }

  /** Every file the app saved in this browser, to keep a copy (or move to another browser). */
  async backup(): Promise<Record<string, ArrayBuffer>> {
    await this.ready;
    return Object.fromEntries(await savedFiles(this.db));
  }

  /** Give the app the files of a backup (the page reloads after). */
  async restore(files: Record<string, ArrayBuffer>) {
    await this.ready;
    await new Promise<void>((resolve, reject) => {
      const tx = this.db.transaction(STORE, "readwrite");
      const store = tx.objectStore(STORE);
      store.clear();
      for (const [path, data] of Object.entries(files)) store.put(data, path);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }

  /** Writes not saved yet are done. */
  flushed(): Promise<void> {
    return this.writes;
  }
}

let backend: Backend | null = null;

/** The backend, started at the first call. */
export function getBackend(): Backend {
  backend ??= new Backend();
  return backend;
}
