/**
 * Bridges an LspClient to Monaco editor by registering language providers
 * for completions, hover, definition, references, signature help, rename,
 * and diagnostics.
 */
import type { Monaco } from '@monaco-editor/react';
import type * as monacoTypes from 'monaco-editor';
import type { LspClient, LspCompletionItem, LspCompletionList, LspDiagnostic, LspHover, LspRange } from './lsp-client';

type MonacoEditor = Parameters<import('@monaco-editor/react').OnMount>[0];
type ITextModel = monacoTypes.editor.ITextModel;
type IPosition = monacoTypes.Position;

const LSP_TO_MONACO_COMPLETION_KIND: Record<number, number> = {
  1: 18,  // Text
  2: 0,   // Method
  3: 1,   // Function
  4: 8,   // Constructor
  5: 4,   // Field
  6: 5,   // Variable
  7: 7,   // Class
  8: 7,   // Interface
  9: 8,   // Module
  10: 9,  // Property
  11: 12, // Unit
  12: 11, // Value
  13: 15, // Enum
  14: 13, // Keyword
  15: 14, // Snippet
  16: 15, // Color
  17: 16, // File
  18: 17, // Reference
  19: 17, // Folder
  20: 15, // EnumMember
  21: 14, // Constant
  22: 7,  // Struct
  23: 18, // Event
  24: 18, // Operator
  25: 18, // TypeParameter
};

const LSP_TO_MONACO_SEVERITY: Record<number, number> = {
  1: 8, // Error
  2: 4, // Warning
  3: 2, // Info
  4: 1, // Hint
};

function lspRangeToMonaco(range: LspRange, monaco: Monaco): InstanceType<typeof monaco.Range> {
  return new monaco.Range(
    range.start.line + 1,
    range.start.character + 1,
    range.end.line + 1,
    range.end.character + 1,
  );
}

function monacoPositionToLsp(position: { lineNumber: number; column: number }): { line: number; character: number } {
  return { line: position.lineNumber - 1, character: position.column - 1 };
}

function markdownContentsToString(
  contents: LspHover['contents'],
): string {
  if (typeof contents === 'string') return contents;
  if ('kind' in contents && 'value' in contents) return (contents as { value: string }).value;
  if (Array.isArray(contents)) {
    return contents
      .map((c) => (typeof c === 'string' ? c : `\`\`\`${c.language}\n${c.value}\n\`\`\``))
      .join('\n\n');
  }
  return '';
}

export interface LspBridgeOptions {
  readonly monaco: Monaco;
  readonly languageId: string;
  readonly client: LspClient;
  readonly onOpenFile?: (path: string, line: number) => void;
}

export interface LspBridge {
  dispose: () => void;
  setModel: (model: ReturnType<MonacoEditor['getModel']>, fileUri?: string | undefined) => void;
  setEditor: (editor: MonacoEditor) => void;
}

/**
 * Create an LSP bridge that registers Monaco providers for the given language.
 * Returns a Disposable that cleans up all registrations.
 */
export function createLspBridge(opts: LspBridgeOptions): LspBridge {
  const { monaco, client } = opts;
  // Monaco uses different language IDs than LSP in some cases.
  // Normalize so providers are registered for the right Monaco language.
  const lspToMonaco: Record<string, string> = {
    typescriptreact: 'typescript',
    javascriptreact: 'javascript',
    jsonc: 'json',
  };
  const languageId = lspToMonaco[opts.languageId] ?? opts.languageId;
  const disposables: Array<{ dispose: () => void }> = [];
  let currentModel: ReturnType<MonacoEditor['getModel']> = null;
  let currentFileUri: string | null = null;

  // --- Diagnostics ---
  client.onNotification('textDocument/publishDiagnostics', (params) => {
    const p = params as { uri: string; diagnostics: LspDiagnostic[] };
    const model = currentModel;
    if (model == null) return;
    if (currentFileUri != null && p.uri !== currentFileUri) return;

    const markers = p.diagnostics.map((d) => ({
      severity: LSP_TO_MONACO_SEVERITY[d.severity ?? 1] ?? 8,
      message: d.message,
      startLineNumber: d.range.start.line + 1,
      startColumn: d.range.start.character + 1,
      endLineNumber: d.range.end.line + 1,
      endColumn: d.range.end.character + 1,
      source: d.source ?? 'lsp',
      code: d.code != null ? String(d.code) : undefined,
    }));

    // Clear Monaco's built-in TS/JS diagnostic markers so only LSP diagnostics show
    monaco.editor.setModelMarkers(model, 'typescript', []);
    monaco.editor.setModelMarkers(model, 'javascript', []);
    monaco.editor.setModelMarkers(model, 'lsp', markers);
  });

  // --- Completion ---
  disposables.push(
    monaco.languages.registerCompletionItemProvider(languageId, {
      triggerCharacters: ['.', '/', '"', "'", '<', ':', '@'],
      provideCompletionItems: async (model: ITextModel, position: IPosition) => {
        try {
          const result = await client.completion(
            fileUri(model),
            monacoPositionToLsp(position),
          );
          if (result == null) return { suggestions: [] };

          const items: LspCompletionItem[] = Array.isArray(result) ? result : (result as LspCompletionList).items;

          return {
            suggestions: items.map((item) => {
              const range = item.textEdit?.range
                ? lspRangeToMonaco(item.textEdit.range, monaco)
                : (undefined as unknown as InstanceType<typeof monaco.Range>);

              return {
                label: item.label,
                kind: LSP_TO_MONACO_COMPLETION_KIND[item.kind ?? 1] ?? 18,
                detail: item.detail,
                documentation: item.documentation != null
                  ? typeof item.documentation === 'string'
                    ? item.documentation
                    : { value: item.documentation.value }
                  : undefined,
                insertText: item.textEdit?.newText ?? item.insertText ?? item.label,
                range,
                sortText: item.sortText,
                filterText: item.filterText,
              };
            }),
          };
        } catch {
          return { suggestions: [] };
        }
      },
    }),
  );

  // --- Hover ---
  disposables.push(
    monaco.languages.registerHoverProvider(languageId, {
      provideHover: async (model: ITextModel, position: IPosition) => {
        try {
          const result = await client.hover(
            fileUri(model),
            monacoPositionToLsp(position),
          );
          if (result == null) return null;

          return {
            range: result.range ? lspRangeToMonaco(result.range, monaco) : undefined,
            contents: [
              {
                value: markdownContentsToString(result.contents),
                isTrusted: true,
              },
            ],
          };
        } catch {
          return null;
        }
      },
    }),
  );

  // Cache the last definition result so the mouseDown handler can navigate
  // cross-file without making a redundant LSP request. The definition provider
  // populates this on Cmd+Hover; the mouseDown handler consumes it on click.
  let lastDefResult: { uri: string; range: LspRange }[] = [];
  let editorMouseDisp: { dispose: () => void } | null = null;

  function uriToPath(uri: string): string {
    return uri.replace(/^file:\/\//, '');
  }

  // Only return same-file locations from providers so Monaco doesn't throw
  // "Model not found" when trying to createModelReference for cross-file URIs.
  function handleLocationResult(
    model: ITextModel,
    result: { uri: string; range: LspRange } | { uri: string; range: LspRange }[] | null,
  ) {
    if (result == null) return null;
    const locations = Array.isArray(result) ? result : [result];
    if (locations.length === 0) return null;

    const currentPath = uriToPath(currentFileUri ?? '');
    const sameFile = locations.filter((l) => uriToPath(l.uri) === currentPath);
    if (sameFile.length === 0) return null;

    return sameFile.map((l) => ({
      uri: model.uri,
      range: lspRangeToMonaco(l.range, monaco),
    }));
  }

  // --- Definition (Cmd+Click) ---
  disposables.push(
    monaco.languages.registerDefinitionProvider(languageId, {
      provideDefinition: async (model: ITextModel, position: IPosition) => {
        try {
          const result = await client.definition(fileUri(model), monacoPositionToLsp(position));
          const allLocs = result != null ? (Array.isArray(result) ? result : [result]) : [];
          lastDefResult = allLocs;
          return handleLocationResult(model, result);
        } catch {
          return null;
        }
      },
    }),
  );

  // --- Type Definition ---
  disposables.push(
    monaco.languages.registerTypeDefinitionProvider(languageId, {
      provideTypeDefinition: async (model: ITextModel, position: IPosition) => {
        try {
          const result = await client.typeDefinition(fileUri(model), monacoPositionToLsp(position));
          return handleLocationResult(model, result);
        } catch {
          return null;
        }
      },
    }),
  );

  // --- Implementation ---
  disposables.push(
    monaco.languages.registerImplementationProvider(languageId, {
      provideImplementation: async (model: ITextModel, position: IPosition) => {
        try {
          const result = await client.implementation(fileUri(model), monacoPositionToLsp(position));
          return handleLocationResult(model, result);
        } catch {
          return null;
        }
      },
    }),
  );

  // --- References ---
  disposables.push(
    monaco.languages.registerReferenceProvider(languageId, {
      provideReferences: async (model: ITextModel, position: IPosition) => {
        try {
          const result = await client.references(
            fileUri(model),
            monacoPositionToLsp(position),
          );
          if (result == null) return null;

          return result.map((loc) => ({
            uri: monaco.Uri.parse(loc.uri),
            range: lspRangeToMonaco(loc.range, monaco),
          }));
        } catch {
          return null;
        }
      },
    }),
  );

  // --- Signature Help ---
  disposables.push(
    monaco.languages.registerSignatureHelpProvider(languageId, {
      signatureHelpTriggerCharacters: ['(', ','],
      provideSignatureHelp: async (model: ITextModel, position: IPosition) => {
        try {
          const result = await client.signatureHelp(
            fileUri(model),
            monacoPositionToLsp(position),
          );
          if (result == null) return null;

          return {
            value: {
              signatures: result.signatures.map((sig) => ({
                label: sig.label,
                documentation: sig.documentation != null
                  ? typeof sig.documentation === 'string'
                    ? sig.documentation
                    : { value: sig.documentation.value }
                  : undefined,
                parameters: (sig.parameters ?? []).map((p) => ({
                  label: p.label,
                  documentation: p.documentation != null
                    ? typeof p.documentation === 'string'
                      ? p.documentation
                      : { value: p.documentation.value }
                    : undefined,
                })),
              })),
              activeSignature: result.activeSignature ?? 0,
              activeParameter: result.activeParameter ?? 0,
            },
            dispose: () => {},
          };
        } catch {
          return null;
        }
      },
    }),
  );

  // --- Rename ---
  disposables.push(
    monaco.languages.registerRenameProvider(languageId, {
      provideRenameEdits: async (model: ITextModel, position: IPosition, newName: string) => {
        try {
          const result = await client.rename(
            fileUri(model),
            monacoPositionToLsp(position),
            newName,
          );
          if (result?.changes == null) return null;

          const edits: Array<{ resource: ReturnType<typeof monaco.Uri.parse>; textEdit: { range: InstanceType<typeof monaco.Range>; text: string } }> = [];
          for (const [uri, changes] of Object.entries(result.changes)) {
            for (const change of changes) {
              edits.push({
                resource: monaco.Uri.parse(uri),
                textEdit: {
                  range: lspRangeToMonaco(change.range, monaco),
                  text: change.newText,
                },
              });
            }
          }

          return { edits };
        } catch {
          return null;
        }
      },
    }),
  );

  function fileUri(model: { uri: { toString: () => string; path?: string; scheme?: string } }): string {
    // Prefer the explicitly tracked file URI
    if (currentFileUri != null) return currentFileUri;
    // With the path prop, model.uri.toString() gives the full URI
    const raw = model.uri.toString();
    if (raw.startsWith('file://')) return raw;
    // Model URI might be just a path (e.g. /workspace/foo.tsx) without scheme
    if (raw.startsWith('/workspace/')) return `file://${raw}`;
    return `file:///workspace/${raw.replace(/^\//, '')}`;
  }

  return {
    dispose: () => {
      for (const d of disposables) d.dispose();
      disposables.length = 0;
      editorMouseDisp?.dispose();
      editorMouseDisp = null;
      if (currentModel != null) {
        monaco.editor.setModelMarkers(currentModel, 'lsp', []);
      }
    },
    setModel: (model, fileUriStr?: string | undefined) => {
      if (currentModel != null) {
        monaco.editor.setModelMarkers(currentModel, 'lsp', []);
      }
      currentModel = model;
      currentFileUri = fileUriStr ?? null;
    },
    setEditor: (ed) => {
      editorMouseDisp?.dispose();
      editorMouseDisp = null;
      if (opts.onOpenFile == null) return;

      const onOpen = opts.onOpenFile;
      editorMouseDisp = ed.onMouseDown((e) => {
        if (!(e.event.metaKey || e.event.ctrlKey)) return;
        if (e.target.type !== monaco.editor.MouseTargetType.CONTENT_TEXT) return;

        const currentPath = uriToPath(currentFileUri ?? '');
        const crossFile = lastDefResult.filter((l) => uriToPath(l.uri) !== currentPath);
        if (crossFile.length > 0) {
          const loc = crossFile[0]!;
          onOpen(uriToPath(loc.uri), loc.range.start.line + 1);
          return;
        }

        // Fallback: if no cached result, make a fresh LSP request
        const pos = e.target.position;
        if (pos == null) return;
        const model = ed.getModel();
        if (model == null) return;

        void (async () => {
          try {
            const result = await client.definition(fileUri(model), monacoPositionToLsp(pos));
            if (result == null) return;
            const locations = Array.isArray(result) ? result : [result];
            const cross = locations.filter((l) => uriToPath(l.uri) !== currentPath);
            if (cross.length > 0) {
              onOpen(uriToPath(cross[0]!.uri), cross[0]!.range.start.line + 1);
            }
          } catch { /* ignore */ }
        })();
      });
    },
  };
}

/**
 * Map file extension to the LSP protocol language ID for textDocument/didOpen.
 * These are the standard LSP language identifiers.
 * Returns null for languages without an LSP server.
 */
export function extToLspLanguage(path: string): string | null {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  const fname = path.split('/').pop()?.toLowerCase() ?? '';

  // Handle special filenames
  if (fname === 'tsconfig.json' || fname === 'jsconfig.json' || fname.endsWith('.jsonc')) return 'jsonc';

  switch (ext) {
    // TypeScript family
    case 'ts': return 'typescript';
    case 'tsx': return 'typescriptreact';
    case 'js': case 'mjs': case 'cjs': return 'javascript';
    case 'jsx': return 'javascriptreact';
    // Web languages (servers from vscode-langservers-extracted)
    case 'json': return 'json';
    case 'css': return 'css';
    case 'scss': return 'scss';
    case 'less': return 'less';
    case 'html': case 'htm': return 'html';
    // YAML
    case 'yaml': case 'yml': return 'yaml';
    // Systems languages
    case 'rs': return 'rust';
    case 'py': return 'python';
    case 'go': return 'go';
    default: return null;
  }
}

/**
 * Normalize LSP language ID to the canonical connection key.
 * Languages that share the same server process use the same connection.
 * e.g. typescript/typescriptreact/javascript/javascriptreact -> one TS server.
 */
export function lspConnectionLanguage(lspLanguage: string): string {
  switch (lspLanguage) {
    case 'javascript': case 'typescript': case 'typescriptreact': case 'javascriptreact':
      return 'typescript';
    case 'jsonc':
      return 'json';
    case 'scss': case 'less':
      return 'css';
    default:
      return lspLanguage;
  }
}
