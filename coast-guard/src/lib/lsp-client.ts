/**
 * Lightweight LSP client that communicates with an LSP server via WebSocket.
 *
 * The daemon's /api/v1/lsp endpoint handles the Content-Length framing
 * translation — we send/receive raw JSON-RPC payloads over the WebSocket.
 */

export interface LspClientOptions {
  readonly project: string;
  readonly name: string;
  readonly language: string;
  readonly rootUri: string;
  readonly rootPath?: string | undefined;
  readonly onDiagnostics?: (uri: string, diagnostics: LspDiagnostic[]) => void;
  readonly onServerReady?: () => void;
  readonly onError?: (msg: string) => void;
  readonly onClose?: () => void;
}

export interface LspPosition {
  readonly line: number;
  readonly character: number;
}

export interface LspRange {
  readonly start: LspPosition;
  readonly end: LspPosition;
}

export interface LspLocation {
  readonly uri: string;
  readonly range: LspRange;
}

export interface LspDiagnostic {
  readonly range: LspRange;
  readonly severity?: number;
  readonly code?: number | string;
  readonly source?: string;
  readonly message: string;
}

export interface LspCompletionItem {
  readonly label: string;
  readonly kind?: number;
  readonly detail?: string;
  readonly documentation?: string | { kind: string; value: string };
  readonly insertText?: string;
  readonly insertTextFormat?: number;
  readonly textEdit?: { range: LspRange; newText: string };
  readonly sortText?: string;
  readonly filterText?: string;
}

export interface LspCompletionList {
  readonly isIncomplete: boolean;
  readonly items: LspCompletionItem[];
}

export interface LspHover {
  readonly contents: string | { kind: string; value: string } | Array<string | { language: string; value: string }>;
  readonly range?: LspRange;
}

export interface LspSignatureHelp {
  readonly signatures: Array<{
    readonly label: string;
    readonly documentation?: string | { kind: string; value: string };
    readonly parameters?: Array<{
      readonly label: string | [number, number];
      readonly documentation?: string | { kind: string; value: string };
    }>;
  }>;
  readonly activeSignature?: number;
  readonly activeParameter?: number;
}

export interface LspWorkspaceEdit {
  readonly changes?: Record<string, Array<{ range: LspRange; newText: string }>>;
}

type PendingRequest = {
  resolve: (result: unknown) => void;
  reject: (error: Error) => void;
};

type NotificationHandler = (params: unknown) => void;

export class LspClient {
  private ws: WebSocket | null = null;
  private nextId = 1;
  private pending = new Map<number, PendingRequest>();
  private handlers = new Map<string, NotificationHandler>();
  private opts: LspClientOptions;
  private ready = false;
  private disposed = false;
  private permanentFailure = false;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectDelay = 1000;
  private serverCapabilities: Record<string, unknown> = {};

  constructor(opts: LspClientOptions) {
    this.opts = opts;

    this.onNotification('textDocument/publishDiagnostics', (params) => {
      const p = params as { uri: string; diagnostics: LspDiagnostic[] };
      opts.onDiagnostics?.(p.uri, p.diagnostics);
    });
  }

  get isReady(): boolean {
    return this.ready;
  }

  get capabilities(): Record<string, unknown> {
    return this.serverCapabilities;
  }

  connect(): void {
    if (this.disposed) return;

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const rp = this.opts.rootPath != null ? `&root_path=${encodeURIComponent(this.opts.rootPath)}` : '';
    const url = `${protocol}//${window.location.host}/api/v1/lsp?project=${encodeURIComponent(this.opts.project)}&name=${encodeURIComponent(this.opts.name)}&language=${encodeURIComponent(this.opts.language)}${rp}`;

    this.ws = new WebSocket(url);

    this.ws.onopen = () => {
      this.reconnectDelay = 1000;
      void this.initialize();
    };

    this.ws.onmessage = (event) => {
      this.handleMessage(event.data as string);
    };

    this.ws.onerror = () => {
      this.opts.onError?.('LSP WebSocket error');
    };

    this.ws.onclose = () => {
      this.ready = false;
      this.rejectAllPending('Connection closed');
      if (!this.disposed && !this.permanentFailure) {
        this.scheduleReconnect();
      }
      this.opts.onClose?.();
    };
  }

  dispose(): void {
    this.disposed = true;
    this.ready = false;
    if (this.reconnectTimer != null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.rejectAllPending('Disposed');
    if (this.ws != null) {
      this.ws.onclose = null;
      this.ws.close();
      this.ws = null;
    }
  }

  // --- LSP lifecycle ---

  private async initialize(): Promise<void> {
    try {
      const result = await this.sendRequest('initialize', {
        processId: null,
        rootUri: this.opts.rootUri,
        capabilities: {
          textDocument: {
            completion: {
              completionItem: {
                snippetSupport: false,
                documentationFormat: ['markdown', 'plaintext'],
              },
            },
            hover: {
              contentFormat: ['markdown', 'plaintext'],
            },
            signatureHelp: {
              signatureInformation: {
                documentationFormat: ['markdown', 'plaintext'],
                parameterInformation: { labelOffsetSupport: true },
              },
            },
            definition: {},
            references: {},
            rename: { prepareSupport: true },
            publishDiagnostics: {
              relatedInformation: false,
            },
            synchronization: {
              didSave: true,
              willSave: false,
              willSaveWaitUntil: false,
            },
          },
          workspace: {
            workspaceFolders: false,
          },
        },
      });

      const r = result as { capabilities?: Record<string, unknown> };
      this.serverCapabilities = r.capabilities ?? {};

      this.sendNotification('initialized', {});
      this.ready = true;
      this.opts.onServerReady?.();
    } catch (e) {
      this.opts.onError?.(`LSP initialize failed: ${e}`);
    }
  }

  // --- Document sync ---

  didOpen(uri: string, languageId: string, version: number, text: string): void {
    if (!this.ready) return;
    this.sendNotification('textDocument/didOpen', {
      textDocument: { uri, languageId, version, text },
    });
  }

  didChange(uri: string, version: number, text: string): void {
    if (!this.ready) return;
    this.sendNotification('textDocument/didChange', {
      textDocument: { uri, version },
      contentChanges: [{ text }],
    });
  }

  didClose(uri: string): void {
    if (!this.ready) return;
    this.sendNotification('textDocument/didClose', {
      textDocument: { uri },
    });
  }

  didSave(uri: string, text?: string): void {
    if (!this.ready) return;
    this.sendNotification('textDocument/didSave', {
      textDocument: { uri },
      ...(text != null ? { text } : {}),
    });
  }

  // --- Feature requests ---

  async completion(uri: string, position: LspPosition): Promise<LspCompletionItem[] | LspCompletionList | null> {
    if (!this.ready) return null;
    const result = await this.sendRequest('textDocument/completion', {
      textDocument: { uri },
      position,
    });
    return result as LspCompletionItem[] | LspCompletionList | null;
  }

  async hover(uri: string, position: LspPosition): Promise<LspHover | null> {
    if (!this.ready) return null;
    const result = await this.sendRequest('textDocument/hover', {
      textDocument: { uri },
      position,
    });
    return result as LspHover | null;
  }

  async definition(uri: string, position: LspPosition): Promise<LspLocation | LspLocation[] | null> {
    if (!this.ready) return null;
    const result = await this.sendRequest('textDocument/definition', {
      textDocument: { uri },
      position,
    });
    return result as LspLocation | LspLocation[] | null;
  }

  async typeDefinition(uri: string, position: LspPosition): Promise<LspLocation | LspLocation[] | null> {
    if (!this.ready) return null;
    const result = await this.sendRequest('textDocument/typeDefinition', {
      textDocument: { uri },
      position,
    });
    return result as LspLocation | LspLocation[] | null;
  }

  async implementation(uri: string, position: LspPosition): Promise<LspLocation | LspLocation[] | null> {
    if (!this.ready) return null;
    const result = await this.sendRequest('textDocument/implementation', {
      textDocument: { uri },
      position,
    });
    return result as LspLocation | LspLocation[] | null;
  }

  async references(uri: string, position: LspPosition): Promise<LspLocation[] | null> {
    if (!this.ready) return null;
    const result = await this.sendRequest('textDocument/references', {
      textDocument: { uri },
      position,
      context: { includeDeclaration: true },
    });
    return result as LspLocation[] | null;
  }

  async signatureHelp(uri: string, position: LspPosition): Promise<LspSignatureHelp | null> {
    if (!this.ready) return null;
    const result = await this.sendRequest('textDocument/signatureHelp', {
      textDocument: { uri },
      position,
    });
    return result as LspSignatureHelp | null;
  }

  async rename(uri: string, position: LspPosition, newName: string): Promise<LspWorkspaceEdit | null> {
    if (!this.ready) return null;
    const result = await this.sendRequest('textDocument/rename', {
      textDocument: { uri },
      position,
      newName,
    });
    return result as LspWorkspaceEdit | null;
  }

  // --- Notification registration ---

  onNotification(method: string, handler: NotificationHandler): void {
    this.handlers.set(method, handler);
  }

  // --- JSON-RPC transport ---

  private sendRequest(method: string, params: unknown): Promise<unknown> {
    return new Promise((resolve, reject) => {
      if (this.ws == null || this.ws.readyState !== WebSocket.OPEN) {
        reject(new Error('WebSocket not connected'));
        return;
      }

      const id = this.nextId++;
      this.pending.set(id, { resolve, reject });

      const msg = JSON.stringify({ jsonrpc: '2.0', id, method, params });
      this.ws.send(msg);

      // Timeout after 30s
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`LSP request '${method}' timed out`));
        }
      }, 30000);
    });
  }

  private sendNotification(method: string, params: unknown): void {
    if (this.ws == null || this.ws.readyState !== WebSocket.OPEN) return;
    const msg = JSON.stringify({ jsonrpc: '2.0', method, params });
    this.ws.send(msg);
  }

  private handleMessage(raw: string): void {
    let msg: { id?: number | null; method?: string; result?: unknown; error?: { code: number; message: string }; params?: unknown };
    try {
      msg = JSON.parse(raw);
    } catch {
      return;
    }

    // Server-sent error with id=null means a fatal/startup error (e.g. binary not found).
    // Stop reconnecting permanently.
    if (msg.id === null && msg.error != null) {
      this.permanentFailure = true;
      this.opts.onError?.(msg.error.message);
      return;
    }

    // Response to a request we sent
    if (msg.id != null && (msg.result !== undefined || msg.error != null)) {
      const pending = this.pending.get(msg.id);
      if (pending != null) {
        this.pending.delete(msg.id);
        if (msg.error != null) {
          pending.reject(new Error(msg.error.message));
        } else {
          pending.resolve(msg.result);
        }
      }
      return;
    }

    // Notification from the server
    if (msg.method != null) {
      const handler = this.handlers.get(msg.method);
      if (handler != null) {
        handler(msg.params);
      }
    }
  }

  private rejectAllPending(reason: string): void {
    for (const [, p] of this.pending) {
      p.reject(new Error(reason));
    }
    this.pending.clear();
  }

  private scheduleReconnect(): void {
    if (this.disposed) return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, this.reconnectDelay);
    this.reconnectDelay = Math.min(this.reconnectDelay * 2, 15000);
  }
}
