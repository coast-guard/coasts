import { useState, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import Modal from './Modal';
import { useSshKeys, useRemoteAddMutation } from '../api/hooks';
import { api } from '../api/endpoints';
import { ApiError } from '../api/client';

interface AddRemoteModalProps {
  readonly open: boolean;
  readonly onAdded: () => void;
  readonly onClose: () => void;
}

const NAME_RE = /^[a-z0-9][a-z0-9-]*$/;

const inputClass =
  'w-full h-9 px-3 text-sm rounded-md border border-[var(--border)] bg-[var(--surface-solid)] dark:bg-transparent text-main outline-none focus:border-[var(--primary)] placeholder:text-subtle-ui';

function parseHostString(raw: string): { user: string; host: string; port: number } {
  let s = raw.trim();
  let user = 'root';
  let port = 22;

  if (s.includes('@')) {
    const atIdx = s.indexOf('@');
    user = s.slice(0, atIdx);
    s = s.slice(atIdx + 1);
  }

  if (s.includes(':')) {
    const colonIdx = s.lastIndexOf(':');
    const portStr = s.slice(colonIdx + 1);
    const parsed = parseInt(portStr, 10);
    if (!isNaN(parsed) && parsed > 0) {
      port = parsed;
      s = s.slice(0, colonIdx);
    }
  }

  return { user, host: s, port };
}

export default function AddRemoteModal({ open, onAdded, onClose }: AddRemoteModalProps) {
  const { t } = useTranslation();
  const { data: sshKeysData } = useSshKeys();
  const addMutation = useRemoteAddMutation();

  const [name, setName] = useState('');
  const [hostString, setHostString] = useState('');
  const [advancedUser, setAdvancedUser] = useState('root');
  const [advancedHost, setAdvancedHost] = useState('');
  const [advancedPort, setAdvancedPort] = useState('22');
  const [sshKey, setSshKey] = useState('');
  const [advanced, setAdvanced] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [keyValid, setKeyValid] = useState<boolean | null>(null);
  const [keyError, setKeyError] = useState<string | null>(null);
  const dragCounter = useRef(0);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const dropZoneRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      setName('');
      setHostString('');
      setAdvancedUser('root');
      setAdvancedHost('');
      setAdvancedPort('22');
      setSshKey('');
      setAdvanced(false);
      setError(null);
      setDragOver(false);
      setKeyValid(null);
      setKeyError(null);
      dragCounter.current = 0;
      return;
    }
    const preventDragOver = (e: Event) => e.preventDefault();
    const preventDrop = (e: Event) => {
      if (dropZoneRef.current?.contains(e.target as Node)) return;
      e.preventDefault();
    };
    document.addEventListener('dragover', preventDragOver);
    document.addEventListener('drop', preventDrop);
    return () => {
      document.removeEventListener('dragover', preventDragOver);
      document.removeEventListener('drop', preventDrop);
    };
  }, [open]);

  useEffect(() => {
    if (!sshKey) {
      setKeyValid(null);
      setKeyError(null);
      return;
    }
    const timer = setTimeout(() => {
      void api.sshKeyValidate(sshKey).then((res) => {
        setKeyValid(res.valid);
        setKeyError(res.error ?? null);
      }).catch(() => {
        setKeyValid(null);
        setKeyError(null);
      });
    }, 400);
    return () => clearTimeout(timer);
  }, [sshKey]);

  const nameError = name.length > 0 && !NAME_RE.test(name);

  const canSubmit = (() => {
    if (!name || nameError) return false;
    if (advanced) {
      return advancedHost.length > 0;
    }
    return hostString.length > 0;
  })();

  const handleSubmit = useCallback(async () => {
    setError(null);

    let user: string;
    let host: string;
    let port: number;

    if (advanced) {
      user = advancedUser || 'root';
      host = advancedHost;
      port = parseInt(advancedPort, 10) || 22;
    } else {
      const parsed = parseHostString(hostString);
      user = parsed.user;
      host = parsed.host;
      port = parsed.port;
    }

    try {
      await addMutation.mutateAsync({
        name,
        host,
        user,
        port,
        ssh_key: sshKey || null,
        sync_strategy: 'mutagen',
      });
      onAdded();
    } catch (e) {
      if (e instanceof ApiError) {
        setError(e.body.error ?? e.message);
      } else {
        setError(String(e));
      }
    }
  }, [name, hostString, advancedUser, advancedHost, advancedPort, sshKey, advanced, addMutation, onAdded]);

  const sshKeys = sshKeysData?.keys ?? [];

  const resolveKeyPath = useCallback(
    (fileName: string) => {
      const match = sshKeys.find((k) => k.name === fileName);
      if (match) {
        setSshKey(match.path);
        return;
      }
      const firstKey = sshKeys[0];
      const sshDir = firstKey != null
        ? firstKey.path.slice(0, firstKey.path.lastIndexOf('/'))
        : '~/.ssh';
      setSshKey(`${sshDir}/${fileName}`);
    },
    [sshKeys],
  );

  return (
    <Modal
      open={open}
      title={t('remote.addTitle')}
      onClose={onClose}
      wide
      actions={
        <>
          <button onClick={onClose} className="btn btn-outline" disabled={addMutation.isPending}>
            {t('action.cancel')}
          </button>
          <button
            disabled={!canSubmit || addMutation.isPending}
            onClick={() => void handleSubmit()}
            className="btn btn-primary"
          >
            {addMutation.isPending ? t('remote.adding') : t('remote.addButton')}
          </button>
        </>
      }
    >
      <div className="space-y-4">
        {/* Name */}
        <div>
          <label className="block text-xs font-medium text-main mb-1.5">
            {t('remote.nameLabel')}
          </label>
          <input
            type="text"
            className={`${inputClass} font-mono ${nameError ? '!border-rose-400 dark:!border-rose-500' : ''}`}
            placeholder={t('remote.namePlaceholder')}
            value={name}
            onChange={(e) => setName(e.target.value.toLowerCase())}
            autoFocus
          />
          {nameError && (
            <p className="mt-1 text-xs text-rose-500">
              Lowercase letters, numbers, and hyphens only.
            </p>
          )}
        </div>

        {/* Host -- simple or advanced */}
        {!advanced ? (
          <div>
            <label className="block text-xs font-medium text-main mb-1.5">
              {t('remote.hostLabel')}
            </label>
            <input
              type="text"
              className={`${inputClass} font-mono`}
              placeholder={t('remote.hostPlaceholder')}
              value={hostString}
              onChange={(e) => setHostString(e.target.value)}
            />
          </div>
        ) : (
          <div className="space-y-3">
            <div className="grid grid-cols-[1fr_auto] gap-3">
              <div>
                <label className="block text-xs font-medium text-main mb-1.5">
                  {t('remote.hostFieldLabel')}
                </label>
                <input
                  type="text"
                  className={`${inputClass} font-mono`}
                  placeholder="192.168.1.100"
                  value={advancedHost}
                  onChange={(e) => setAdvancedHost(e.target.value)}
                />
              </div>
              <div className="w-20">
                <label className="block text-xs font-medium text-main mb-1.5">
                  {t('remote.portLabel')}
                </label>
                <input
                  type="text"
                  className={`${inputClass} font-mono`}
                  placeholder="22"
                  value={advancedPort}
                  onChange={(e) => setAdvancedPort(e.target.value)}
                />
              </div>
            </div>
            <div>
              <label className="block text-xs font-medium text-main mb-1.5">
                {t('remote.userLabel')}
              </label>
              <input
                type="text"
                className={`${inputClass} font-mono`}
                placeholder="root"
                value={advancedUser}
                onChange={(e) => setAdvancedUser(e.target.value)}
              />
            </div>
          </div>
        )}

        {/* Advanced toggle */}
        <button
          type="button"
          className="text-xs text-[var(--primary)] hover:underline"
          onClick={() => setAdvanced((v) => !v)}
        >
          {advanced ? '← Simple' : t('remote.advanced') + ' →'}
        </button>

        {/* SSH Key */}
        <div>
          <label className="block text-xs font-medium text-main mb-1.5">
            {t('remote.sshKeyLabel')}
          </label>

          <div
            ref={dropZoneRef}
            className={`rounded-lg border-2 border-dashed transition-colors p-3 ${
              dragOver
                ? 'border-[var(--primary)] bg-[var(--primary)]/5'
                : 'border-[var(--border)]'
            }`}
            onDragEnter={(e) => {
              e.preventDefault();
              dragCounter.current += 1;
              if (dragCounter.current === 1) {
                setDragOver(true);
              }
            }}
            onDragOver={(e) => {
              e.preventDefault();
            }}
            onDragLeave={(e) => {
              e.preventDefault();
              dragCounter.current -= 1;
              if (dragCounter.current === 0) {
                setDragOver(false);
              }
            }}
            onDrop={(e) => {
              e.preventDefault();
              dragCounter.current = 0;
              setDragOver(false);
              const file = e.dataTransfer.files[0];
              if (file) {
                resolveKeyPath(file.name);
              }
            }}
          >
            {/* Path input + browse */}
            <div className="flex gap-2">
              <input
                type="text"
                className={`${inputClass} font-mono text-xs flex-1 ${
                  sshKey && keyValid === true ? '!border-green-500' : ''
                }${sshKey && keyError ? '!border-rose-400 dark:!border-rose-500' : ''}`}
                placeholder={t('remote.sshKeyPlaceholder')}
                value={sshKey}
                onChange={(e) => setSshKey(e.target.value)}
              />
              <button
                type="button"
                className="shrink-0 h-9 px-3 text-xs font-medium rounded-md border border-[var(--border)] text-main bg-[var(--surface-solid)] dark:bg-transparent hover:bg-[var(--surface-hover)] transition-colors"
                onClick={() => fileInputRef.current?.click()}
              >
                Browse
              </button>
              <input
                ref={fileInputRef}
                type="file"
                className="hidden"
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) {
                    resolveKeyPath(file.name);
                  }
                  e.target.value = '';
                }}
              />
            </div>

            {/* Detected keys as clickable chips */}
            {sshKeys.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {sshKeys.map((k) => (
                  <button
                    key={k.path}
                    type="button"
                    className={`inline-flex items-center gap-1 px-2.5 py-1 text-xs font-mono rounded-md border transition-colors ${
                      sshKey === k.path
                        ? 'border-[var(--primary)] bg-[var(--primary)]/10 text-[var(--primary)]'
                        : 'border-[var(--border)] text-subtle-ui hover:text-main hover:border-[var(--primary)] hover:bg-[var(--surface-hover)]'
                    }`}
                    onClick={() => setSshKey(sshKey === k.path ? '' : k.path)}
                    title={k.path}
                  >
                    {k.name}
                  </button>
                ))}
              </div>
            )}

            {dragOver && (
              <p className="mt-2 text-xs text-[var(--primary)] text-center font-medium">
                Drop key file here
              </p>
            )}
          </div>

          {sshKey === '' ? (
            <p className="mt-1.5 text-xs text-subtle-ui">{t('remote.sshKeyNone')}</p>
          ) : keyValid === true ? (
            <p className="mt-1.5 text-xs text-green-600 dark:text-green-400">Valid private key</p>
          ) : keyError != null ? (
            <p className="mt-1.5 text-xs text-rose-500">{keyError}</p>
          ) : null}
        </div>

        {/* Error */}
        {error != null && (
          <div className="p-3 rounded-md bg-rose-500/10 text-sm text-rose-600 dark:text-rose-400">
            {error}
          </div>
        )}
      </div>
    </Modal>
  );
}
