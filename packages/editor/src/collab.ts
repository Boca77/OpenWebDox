import type { EditorView, RemoteSelection } from "./view.js";

/** Bytes from the engine: always a fresh copy out of wasm memory, never shared. */
type Bytes = Uint8Array<ArrayBuffer>;

/** What `Collaboration` needs from the engine. The wasm `Editor` implements it. */
export interface SyncEngine {
  sync_connect(): Bytes;
  sync_disconnected(): void;
  sync_is_synced(): boolean;
  sync_flush(): Bytes | undefined;
  /** `[flags, anchor, focus]`; flags: 1 = document changed, 2 = remote carets changed. */
  sync_receive(message: Uint8Array, anchor: number, focus: number): Uint32Array;
  sync_presence(name: string, color: string, anchor: number, focus: number, visible: boolean): Bytes;
  peer_ids(): Uint32Array;
  peer_name(id: number): string | undefined;
  peer_color(id: number): string | undefined;
  /** `[anchor, focus]` */
  peer_selection(id: number): Uint32Array | undefined;
}

export type ConnectionState = "connecting" | "connected" | "offline";

export interface Peer {
  id: number;
  name: string;
  color: string;
}

export interface CollaborationOptions {
  /** Shown to others next to this user's caret. */
  name: string;
  /** CSS colour for this user's caret, dark enough for white text. */
  color: string;
  /** Local edits are batched for this long before being sent (default 30 ms). */
  flushMs?: number;
  /** Caret moves are sent at most this often (default 60 ms). */
  presenceMs?: number;
}

const DOC_CHANGED = 1;
const PEERS_CHANGED = 2;
const RETRY_MIN_MS = 500;
const RETRY_MAX_MS = 8000;

/**
 * Keeps an editor in sync with an opendox server over a WebSocket.
 *
 * Editing never waits for the network: edits apply locally at once and go out in batches. If
 * the connection drops, editing carries on and the session reconnects with backoff; the
 * handshake on reconnect exchanges whatever each side missed.
 */
export class Collaboration {
  /** Called when the connection state or the list of people changes. */
  onStatus?: (state: ConnectionState, peers: Peer[]) => void;

  private ws: WebSocket | null = null;
  private state: ConnectionState = "offline";
  private stopped = true;
  private retryMs = RETRY_MIN_MS;
  private retryTimer = 0;
  private flushTimer = 0;
  private presenceTimer = 0;
  private lastPresence = "";
  private readonly flushMs: number;
  private readonly presenceMs: number;

  constructor(
    private readonly engine: SyncEngine,
    private readonly view: EditorView,
    private readonly url: string,
    private readonly user: CollaborationOptions,
  ) {
    this.flushMs = user.flushMs ?? 30;
    this.presenceMs = user.presenceMs ?? 60;
    view.remoteSelections = () => this.remoteSelections();
    view.on("edit", () => this.scheduleFlush());
    view.on("selection", () => this.schedulePresence());
  }

  get connectionState(): ConnectionState {
    return this.state;
  }

  /** People currently on the document, other than this user. */
  peers(): Peer[] {
    return [...this.engine.peer_ids()].map((id) => ({
      id,
      name: this.engine.peer_name(id) ?? "",
      color: this.engine.peer_color(id) ?? "currentColor",
    }));
  }

  /** Opens the connection, and keeps reopening it after drops until `disconnect()`. */
  connect(): void {
    this.stopped = false;
    this.open();
  }

  /** Closes the connection and stops reconnecting. Local editing carries on. */
  disconnect(): void {
    this.stopped = true;
    clearTimeout(this.retryTimer);
    this.ws?.close();
    this.closed();
  }

  private open(): void {
    if (this.ws || this.stopped) return;
    const ws = new WebSocket(this.url);
    ws.binaryType = "arraybuffer";
    this.ws = ws;
    this.setState("connecting");
    ws.onopen = () => ws.send(this.engine.sync_connect());
    ws.onmessage = (e) => this.receive(new Uint8Array(e.data as ArrayBuffer));
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.closed();
      if (!this.stopped) {
        this.retryTimer = window.setTimeout(() => this.open(), this.retryMs);
        this.retryMs = Math.min(this.retryMs * 2, RETRY_MAX_MS);
      }
    };
  }

  private closed(): void {
    this.ws = null;
    this.lastPresence = "";
    this.engine.sync_disconnected();
    this.setState("offline");
    // Remote carets are gone; redraw without them.
    const { anchor, focus } = this.view.selection;
    this.view.applyRemote(anchor, focus, false);
  }

  private receive(message: Uint8Array): void {
    const wasSynced = this.engine.sync_is_synced();
    const { anchor, focus } = this.view.selection;
    const [flags, a, f] = this.engine.sync_receive(message, anchor, focus);
    if (flags & (DOC_CHANGED | PEERS_CHANGED)) {
      this.view.applyRemote(a, f, (flags & DOC_CHANGED) !== 0);
    }
    this.flush();
    if (!wasSynced && this.engine.sync_is_synced()) {
      this.retryMs = RETRY_MIN_MS;
      this.setState("connected");
      this.sendPresence();
    } else if (flags & PEERS_CHANGED) {
      this.onStatus?.(this.state, this.peers());
    }
  }

  private scheduleFlush(): void {
    if (!this.flushTimer) {
      this.flushTimer = window.setTimeout(() => {
        this.flushTimer = 0;
        this.flush();
      }, this.flushMs);
    }
  }

  /** Sends everything waiting: handshake replies and batched local edits. */
  private flush(): void {
    const ws = this.ws;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    for (let m = this.engine.sync_flush(); m; m = this.engine.sync_flush()) ws.send(m);
  }

  private schedulePresence(): void {
    if (!this.presenceTimer) {
      this.presenceTimer = window.setTimeout(() => {
        this.presenceTimer = 0;
        this.sendPresence();
      }, this.presenceMs);
    }
  }

  private sendPresence(): void {
    const ws = this.ws;
    if (!ws || ws.readyState !== WebSocket.OPEN || !this.engine.sync_is_synced()) return;
    const { anchor, focus } = this.view.selection;
    const key = `${anchor}:${focus}`;
    if (key === this.lastPresence) return;
    // Edits first, so others have the text this caret points into.
    this.flush();
    ws.send(this.engine.sync_presence(this.user.name, this.user.color, anchor, focus, true));
    this.lastPresence = key;
  }

  private remoteSelections(): RemoteSelection[] {
    const out: RemoteSelection[] = [];
    for (const p of this.peers()) {
      const sel = this.engine.peer_selection(p.id);
      if (sel) out.push({ name: p.name, color: p.color, anchor: sel[0], focus: sel[1] });
    }
    return out;
  }

  private setState(state: ConnectionState): void {
    this.state = state;
    this.onStatus?.(state, this.peers());
  }
}
