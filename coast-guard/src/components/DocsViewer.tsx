import { useMemo, useState, useCallback } from 'react';
import { createPortal } from 'react-dom';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import hljs from 'highlight.js/lib/core';
import hljsToml from 'highlight.js/lib/languages/ini';
import hljsBash from 'highlight.js/lib/languages/bash';
import { useNavigate } from 'react-router';
import type { Components } from 'react-markdown';
import { X, Copy, Check, ArrowsOut } from '@phosphor-icons/react';
import { api } from '../api/endpoints';

hljs.registerLanguage('toml', hljsToml);
hljs.registerLanguage('bash', hljsBash);

const HIGHLIGHT_LANGUAGES = new Set(['toml', 'bash']);

interface DocsViewerProps {
  content: string;
  basePath: string;
  files?: Record<string, string>;
}

function resolveRelativePath(path: string, basePath: string): string {
  const parts = basePath.split('/').filter(Boolean);
  const pathParts = path.split('/');

  for (const p of pathParts) {
    if (p === '..') {
      parts.pop();
    } else if (p !== '.') {
      parts.push(p);
    }
  }

  return parts.join('/');
}

function resolveDocLink(href: string, basePath: string): string | null {
  if (href.startsWith('http://') || href.startsWith('https://') || href.startsWith('#')) {
    return null;
  }
  if (!href.endsWith('.md')) return null;

  const resolved = resolveRelativePath(href.replace(/\.md$/, ''), basePath);
  return '/docs/' + resolved;
}

function resolveTxtPath(href: string, basePath: string): string | null {
  if (href.startsWith('http://') || href.startsWith('https://') || href.startsWith('#')) {
    return null;
  }
  if (!href.endsWith('.txt')) return null;

  return resolveRelativePath(href, basePath);
}

function resolveImageSrc(src: string, basePath: string): string {
  if (
    src.startsWith('http://') ||
    src.startsWith('https://') ||
    src.startsWith('data:') ||
    src.startsWith('blob:') ||
    src.startsWith('/')
  ) {
    return src;
  }

  const resolved = resolveRelativePath(src, basePath);
  if (resolved.startsWith('assets/')) {
    return `/docs-assets/${resolved.slice('assets/'.length)}`;
  }

  return src;
}

function TextFileModal({ filename, content, onClose }: { filename: string; content: string; onClose: () => void }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    void navigator.clipboard.writeText(content).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [content]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--overlay)] backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="relative w-full max-w-3xl max-h-[80vh] mx-4 rounded-xl border border-[var(--border)] shadow-2xl flex flex-col bg-[var(--surface-solid)]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-3 border-b border-[var(--border)]">
          <span className="text-sm font-semibold text-main font-mono">{filename}</span>
          <div className="flex items-center gap-2">
            <button
              onClick={handleCopy}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md transition-colors bg-[var(--surface-muted)] hover:bg-[var(--surface-muted-hover)] text-main border border-[var(--border)]"
            >
              {copied ? <Check size={14} /> : <Copy size={14} />}
              {copied ? 'Copied' : 'Copy'}
            </button>
            <button
              onClick={onClose}
              className="p-1 rounded-md transition-colors hover:bg-[var(--surface-muted)] text-muted-ui hover:text-main"
            >
              <X size={16} />
            </button>
          </div>
        </div>
        <div className="overflow-auto p-5">
          <pre className="whitespace-pre-wrap text-sm font-mono leading-relaxed text-muted-ui">
            {content}
          </pre>
        </div>
      </div>
    </div>
  );
}

function ImageLightbox({ src, alt, onClose }: { src: string; alt: string; onClose: () => void }) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--overlay-strong)] backdrop-blur-sm cursor-zoom-out"
      onClick={onClose}
    >
      <button
        onClick={onClose}
        className="absolute top-4 right-4 p-2 rounded-lg bg-[var(--overlay)] text-[var(--code-block-text)] hover:bg-[var(--overlay-strong)] transition-colors"
      >
        <X size={20} />
      </button>
      <img
        src={src}
        alt={alt}
        className="max-w-[90vw] max-h-[90vh] rounded-lg shadow-2xl object-contain"
        onClick={(e) => e.stopPropagation()}
      />
    </div>
  );
}

function extractText(node: React.ReactNode): string {
  if (typeof node === 'string') return node;
  if (typeof node === 'number') return String(node);
  if (node == null || typeof node === 'boolean') return '';
  if (Array.isArray(node)) return node.map(extractText).join('');
  if (typeof node === 'object' && 'props' in node) {
    const el = node as React.ReactElement<{ children?: React.ReactNode }>;
    return extractText(el.props.children);
  }
  return '';
}

function CopyableCodeBlock({
  text,
  preClassName,
  preProps,
  children,
}: {
  text: string;
  preClassName: string;
  preProps: Record<string, unknown>;
  children: React.ReactNode;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [text]);

  return (
    <div className="relative group">
      <button
        onClick={handleCopy}
        className="absolute top-2 right-2 inline-flex items-center gap-1 px-2 py-1 text-xs rounded-md transition-opacity opacity-0 group-hover:opacity-100 bg-[var(--surface-muted)] hover:bg-[var(--surface-muted-hover)] text-muted-ui hover:text-main border border-[var(--border)]"
      >
        {copied ? <Check size={12} /> : <Copy size={12} />}
        {copied ? 'Copied' : 'Copy'}
      </button>
      <pre className={preClassName} {...preProps}>
        {children}
      </pre>
    </div>
  );
}

export default function DocsViewer({ content, basePath, files }: DocsViewerProps) {
  const navigate = useNavigate();
  const [modalFile, setModalFile] = useState<{ filename: string; content: string } | null>(null);
  const [lightboxImg, setLightboxImg] = useState<{ src: string; alt: string } | null>(null);

  const components = useMemo<Components>(() => ({
    a({ href, children, ...rest }) {
      if (href == null) return <a {...rest}>{children}</a>;

      const txtPath = resolveTxtPath(href, basePath);
      const txtContent = txtPath != null ? files?.[txtPath] : undefined;
      if (txtPath != null && txtContent != null) {
        const filename = href.split('/').pop() ?? href;
        return (
          <a
            href="#"
            onClick={(e) => {
              e.preventDefault();
              api.track('docs_txt_file_open', {
                from_base_path: basePath,
                link_href: href,
                resolved_path: txtPath,
              });
              setModalFile({ filename, content: txtContent });
            }}
            className="text-[var(--primary)] hover:text-[var(--primary-strong)] underline underline-offset-2"
            {...rest}
          >
            {children}
          </a>
        );
      }

      const resolved = resolveDocLink(href, basePath);
      if (resolved != null) {
        return (
          <a
            href={`#${resolved}`}
            onClick={(e) => {
              e.preventDefault();
              api.track('docs_internal_link_open', {
                from_base_path: basePath,
                link_href: href,
                route: resolved,
              });
              void navigate(resolved);
            }}
            className="text-[var(--primary)] hover:text-[var(--primary-strong)] underline underline-offset-2"
            {...rest}
          >
            {children}
          </a>
        );
      }

      return (
        <a
          href={href}
          target="_blank"
          rel="noopener noreferrer"
          className="text-[var(--primary)] hover:text-[var(--primary-strong)] underline underline-offset-2"
          {...rest}
        >
          {children}
        </a>
      );
    },
    img({ src, alt, ...rest }) {
      const resolvedSrc = typeof src === 'string' ? resolveImageSrc(src, basePath) : src;
      const altText = alt ?? '';
      return (
        <span className="group relative inline-block mb-4">
          <img
            src={resolvedSrc}
            alt={altText}
            loading="lazy"
            className="max-w-full rounded-lg border border-[var(--border)] cursor-zoom-in"
            onClick={() => {
              if (typeof resolvedSrc === 'string') setLightboxImg({ src: resolvedSrc, alt: altText });
            }}
            {...rest}
          />
          <button
            onClick={() => {
              if (typeof resolvedSrc === 'string') setLightboxImg({ src: resolvedSrc, alt: altText });
            }}
            className="absolute top-2 right-2 p-1.5 rounded-md bg-[var(--overlay)] text-[var(--code-block-text)] opacity-0 group-hover:opacity-100 transition-opacity hover:bg-[var(--overlay-strong)]"
            title="Expand image"
          >
            <ArrowsOut size={16} />
          </button>
        </span>
      );
    },
    h1({ children, ...rest }) {
      return <h1 className="text-2xl font-bold mb-4 mt-6 first:mt-0 text-main" {...rest}>{children}</h1>;
    },
    h2({ children, ...rest }) {
      return <h2 className="text-xl font-semibold mb-3 mt-6 text-main border-b border-[var(--border)] pb-2" {...rest}>{children}</h2>;
    },
    h3({ children, ...rest }) {
      return <h3 className="text-lg font-semibold mb-2 mt-5 text-main" {...rest}>{children}</h3>;
    },
    p({ children, ...rest }) {
      return <p className="mb-3 leading-relaxed text-muted-ui" {...rest}>{children}</p>;
    },
    ul({ children, ...rest }) {
      return <ul className="mb-3 ml-5 list-disc text-muted-ui space-y-1" {...rest}>{children}</ul>;
    },
    ol({ children, ...rest }) {
      return <ol className="mb-3 ml-5 list-decimal text-muted-ui space-y-1" {...rest}>{children}</ol>;
    },
    li({ children, ...rest }) {
      return <li className="leading-relaxed" {...rest}>{children}</li>;
    },
    code({ className, children, ...rest }) {
      const isBlock = className?.startsWith('language-');
      if (isBlock) {
        const cleaned = className?.replace(/-emphasis$/, '') ?? '';
        const lang = cleaned.replace('language-', '');
        if (HIGHLIGHT_LANGUAGES.has(lang)) {
          const raw = String(children).replace(/\n$/, '');
          const highlighted = hljs.highlight(raw, { language: lang }).value;
          return (
            <code
              className={`hljs ${cleaned} block`}
              dangerouslySetInnerHTML={{ __html: highlighted }}
              {...rest}
            />
          );
        }
        return (
          <code className={`${cleaned} block`} {...rest}>
            {children}
          </code>
        );
      }
      return (
        <code
          className="px-1.5 py-0.5 rounded-md text-sm font-mono bg-[var(--surface-muted)] border border-[var(--border)] text-[var(--primary)]"
          {...rest}
        >
          {children}
        </code>
      );
    },
    pre({ children, ...rest }) {
      const childProps = (children as React.ReactElement | undefined)?.props as Record<string, unknown> | undefined;
      const childClass = typeof childProps?.['className'] === 'string' ? childProps['className'] : '';
      const isEmphasis = childClass.includes('-emphasis');
      const isCopyable = childClass.includes('-copy');

      const preClassName = isEmphasis
        ? 'mb-4 p-4 rounded-lg overflow-x-auto font-mono text-sm leading-relaxed bg-[var(--docs-code-surface)] border-2 border-[var(--primary)] ring-1 ring-[var(--primary)]/20'
        : 'mb-4 p-4 rounded-lg overflow-x-auto font-mono text-sm leading-relaxed bg-[var(--docs-code-surface)] border border-[var(--border)]';

      if (isCopyable) {
        const textContent = typeof childProps?.['children'] === 'string'
          ? childProps['children']
          : extractText(children);
        return (
          <CopyableCodeBlock text={textContent} preClassName={preClassName} preProps={rest}>
            {children}
          </CopyableCodeBlock>
        );
      }

      return (
        <pre className={preClassName} {...rest}>
          {children}
        </pre>
      );
    },
    table({ children, ...rest }) {
      return (
        <div className="mb-4 overflow-x-auto rounded-lg border border-[var(--border)]">
          <table className="w-full text-sm" {...rest}>{children}</table>
        </div>
      );
    },
    thead({ children, ...rest }) {
      return <thead className="bg-[var(--surface-muted)]" {...rest}>{children}</thead>;
    },
    th({ children, ...rest }) {
      return <th className="px-4 py-2 text-left font-semibold text-main border-b border-[var(--border)]" {...rest}>{children}</th>;
    },
    td({ children, ...rest }) {
      return <td className="px-4 py-2 text-muted-ui border-b border-[var(--border)]" {...rest}>{children}</td>;
    },
    strong({ children, ...rest }) {
      return <strong className="font-semibold text-main" {...rest}>{children}</strong>;
    },
    blockquote({ children, ...rest }) {
      return (
        <blockquote
          className="mb-3 pl-4 border-l-4 border-[var(--primary)] text-muted-ui italic"
          {...rest}
        >
          {children}
        </blockquote>
      );
    },
    hr(rest) {
      return <hr className="my-6 border-[var(--border)]" {...rest} />;
    },
  }), [basePath, navigate]);

  return (
    <div className="docs-viewer max-w-none">
      <Markdown remarkPlugins={[remarkGfm]} components={components}>
        {content}
      </Markdown>
      {modalFile != null && createPortal(
        <TextFileModal
          filename={modalFile.filename}
          content={modalFile.content}
          onClose={() => setModalFile(null)}
        />,
        document.body,
      )}
      {lightboxImg != null && createPortal(
        <ImageLightbox
          src={lightboxImg.src}
          alt={lightboxImg.alt}
          onClose={() => setLightboxImg(null)}
        />,
        document.body,
      )}
    </div>
  );
}
