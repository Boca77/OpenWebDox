import type { EditorView } from "./view.js";

/** What `LocalCopy` needs from the engine. The wasm `Editor` implements it. */
export interface StateEngine {
  save_state(): Uint8Array;
  load_state(bytes: Uint8Array): void;
}

const DB = "opendox";
const STORE = "documents";

/**
 * A copy of a shared document kept on this device (IndexedDB), so edits made offline survive
 * closing the tab: load it before connecting, and the sync handshake sends the server
 * whatever it lacks. Saved shortly after every change (local or remote) and whenever the page
 * is hidden.
 *
 * The copy holds the document's full edit history, so it merges with the server's copy rather
 * than replacing it.
 */
export class LocalCopy {
  private timer = 0;
  private dirty = false;
  private writes: Promise<void> = Promise.resolve();
  /** Called after each save, with whether it worked. */
  onSaved?: (ok: boolean) => void;

  private constructor(
    private readonly db: IDBDatabase,
    private readonly key: string,
    private readonly engine: StateEngine,
    private readonly delayMs: number,
  ) {}

  /**
   * Opens the copy of document `id`, merging it into `engine` if one was saved before. Returns
   * `null` where IndexedDB isn't available (private windows in some browsers): editing still
   * works, only without the offline copy.
   */
  static async open(engine: StateEngine, id: string, delayMs = 300): Promise<LocalCopy | null> {
    const db = await openDb().catch(() => null);
    if (!db) return null;
    const copy = new LocalCopy(db, id, engine, delayMs);
    const saved = await copy.read().catch(() => undefined);
    if (saved) engine.load_state(saved);
    return copy;
  }

  /** Saves after changes in `view`, and when the page is hidden or closed. */
  attach(view: EditorView): void {
    view.on("edit", () => this.schedule());
    view.on("remote", () => this.schedule());
    const now = () => {
      if (this.dirty) void this.save();
    };
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") now();
    });
    window.addEventListener("pagehide", now);
  }

  /** Writes the document now. Writes happen one at a time, in order. */
  save(): Promise<void> {
    clearTimeout(this.timer);
    this.timer = 0;
    this.dirty = false;
    let bytes: Uint8Array;
    try {
      bytes = this.engine.save_state();
    } catch {
      this.onSaved?.(false);
      return this.writes;
    }
    this.writes = this.writes.then(() =>
      this.write(bytes).then(
        () => this.onSaved?.(true),
        () => this.onSaved?.(false),
      ),
    );
    return this.writes;
  }

  /** Forgets this device's copy (e.g. when the user signs out). */
  async clear(): Promise<void> {
    await this.writes;
    await request(this.db.transaction(STORE, "readwrite").objectStore(STORE).delete(this.key));
  }

  private schedule(): void {
    this.dirty = true;
    if (!this.timer) this.timer = window.setTimeout(() => void this.save(), this.delayMs);
  }

  private async read(): Promise<Uint8Array | undefined> {
    const v = await request(this.db.transaction(STORE, "readonly").objectStore(STORE).get(this.key));
    return v instanceof Uint8Array ? v : undefined;
  }

  private write(bytes: Uint8Array): Promise<unknown> {
    const tx = this.db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(bytes, this.key);
    return new Promise((resolve, reject) => {
      tx.oncomplete = () => resolve(undefined);
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error);
    });
  }
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function request<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}
