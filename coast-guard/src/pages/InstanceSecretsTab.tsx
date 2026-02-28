import { useMemo, useState, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Eye, PencilSimple } from '@phosphor-icons/react';
import { useQueryClient } from '@tanstack/react-query';
import type { ProjectName, InstanceName } from '../types/branded';
import type { SecretInfo } from '../types/api';
import { useSecrets, qk } from '../api/hooks';
import { api } from '../api/endpoints';
import DataTable, { type Column } from '../components/DataTable';
import Modal from '../components/Modal';
import HighlightedValue from '../components/HighlightedValue';

interface Props {
  readonly project: ProjectName;
  readonly name: InstanceName;
  readonly buildId?: string | null;
}

export default function InstanceSecretsTab({ project, name, buildId }: Props) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const { data, isLoading, error } = useSecrets(project, name);

  const [modalSecret, setModalSecret] = useState<string | null>(null);
  const [modalValue, setModalValue] = useState<string | null>(null);
  const [modalLoading, setModalLoading] = useState(false);
  const [modalError, setModalError] = useState<string | null>(null);

  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState('');
  const [saving, setSaving] = useState(false);
  const [saveSuccess, setSaveSuccess] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [rerunning, setRerunning] = useState(false);
  const [rerunError, setRerunError] = useState<string | null>(null);
  const [rerunSuccess, setRerunSuccess] = useState<string | null>(null);

  const secrets = data ?? [];

  const handleReveal = useCallback(async (secretName: string) => {
    setModalSecret(secretName);
    setModalValue(null);
    setModalError(null);
    setModalLoading(true);
    setEditing(false);
    setSaveSuccess(false);
    try {
      const res = await api.revealSecret(project, name, secretName);
      setModalValue(res.value);
    } catch {
      setModalError(t('secrets.revealError'));
    } finally {
      setModalLoading(false);
    }
  }, [project, name, t]);

  const closeModal = useCallback(() => {
    setModalSecret(null);
    setModalValue(null);
    setModalError(null);
    setEditing(false);
    setSaveSuccess(false);
  }, []);

  const startEditing = useCallback(() => {
    setEditValue(modalValue ?? '');
    setEditing(true);
    setSaveSuccess(false);
    requestAnimationFrame(() => textareaRef.current?.focus());
  }, [modalValue]);

  const cancelEditing = useCallback(() => {
    setEditing(false);
  }, []);

  const handleSave = useCallback(async () => {
    if (modalSecret == null) return;
    setSaving(true);
    setSaveSuccess(false);
    setModalError(null);
    try {
      await api.overrideSecret(project, name, modalSecret, editValue);
      setModalValue(editValue);
      setEditing(false);
      setSaveSuccess(true);
      void queryClient.invalidateQueries({ queryKey: qk.secrets(project, name) });
      setTimeout(() => setSaveSuccess(false), 3000);
    } catch {
      setModalError(t('secrets.saveError'));
    } finally {
      setSaving(false);
    }
  }, [project, name, modalSecret, editValue, queryClient, t]);

  const handleRerunExtractors = useCallback(async () => {
    setRerunning(true);
    setRerunError(null);
    setRerunSuccess(null);
    try {
      const result = await api.rerunExtractors(project as string, buildId);
      if (result.error != null) {
        setRerunError(t('secrets.rerunError', { error: result.error.error }));
        return;
      }
      const extracted = result.complete?.secrets_extracted ?? 0;
      setRerunSuccess(t('secrets.rerunSuccess', { count: extracted }));
      void queryClient.invalidateQueries({ queryKey: qk.secrets(project, name) });
    } catch (e) {
      setRerunError(t('secrets.rerunError', { error: String(e) }));
    } finally {
      setRerunning(false);
    }
  }, [project, name, buildId, queryClient, t]);

  const columns: readonly Column<SecretInfo>[] = useMemo(
    () => [
      {
        key: 'name',
        header: t('col.name'),
        render: (r) => <span className="font-mono text-xs">{r.name}</span>,
      },
      {
        key: 'extractor',
        header: t('col.extractor'),
        render: (r) => <span className="text-xs">{r.extractor}</span>,
      },
      {
        key: 'inject',
        header: t('col.inject'),
        render: (r) => (
          <div className="flex items-center gap-2">
            <span className="font-mono text-xs text-subtle-ui">{r.inject}</span>
            <button
              onClick={(e) => { e.stopPropagation(); void handleReveal(r.name); }}
              className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium
                bg-white/5 border border-[var(--border)] text-[var(--primary)]
                hover:bg-white/10 transition-colors"
            >
              <Eye size={12} />
              {t('secrets.show')}
            </button>
          </div>
        ),
      },
      {
        key: 'is_override',
        header: t('col.override'),
        render: (r) =>
          r.is_override ? (
            <span className="inline-block px-2 py-0.5 rounded-full text-[10px] font-semibold bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20">
              {t('secrets.overrideYes')}
            </span>
          ) : (
            <span className="text-xs text-subtle-ui">{t('secrets.overrideNo')}</span>
          ),
      },
    ],
    [t, i18n.language, handleReveal],
  );

  if (isLoading) return <p className="text-sm text-subtle-ui py-4">{t('secrets.loading')}</p>;
  if (error != null) return <p className="text-sm text-rose-500 py-4">{t('secrets.loadError', { error: String(error) })}</p>;

  return (
    <>
      <div className="glass-panel overflow-hidden">
        <div className="flex items-center gap-2 flex-wrap px-4 py-2 bg-white/20 dark:bg-white/5 border-b border-[var(--border)]">
          <button
            onClick={() => void handleRerunExtractors()}
            disabled={rerunning}
            className="btn btn-outline disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {rerunning ? t('secrets.rerunRunning') : t('secrets.rerun')}
          </button>
          {rerunSuccess != null && (
            <span className="text-xs text-emerald-500">{rerunSuccess}</span>
          )}
          {rerunError != null && (
            <span className="text-xs text-rose-500">{rerunError}</span>
          )}
          <span className="ml-auto text-xs text-subtle-ui">
            {t('toolbar.total', { count: secrets.length })}
          </span>
        </div>
        <DataTable
          columns={columns}
          data={secrets as SecretInfo[]}
          getRowId={(r) => r.name}
          emptyMessage={t('secrets.empty')}
        />
      </div>

      <Modal
        open={modalSecret != null}
        title={modalSecret ?? ''}
        onClose={closeModal}
        actions={
          !modalLoading && modalValue != null && !editing ? (
            <button
              onClick={startEditing}
              className="btn btn-outline inline-flex items-center gap-1.5 text-xs"
            >
              <PencilSimple size={14} />
              {t('secrets.override')}
            </button>
          ) : undefined
        }
      >
        {modalLoading && (
          <p className="text-sm text-subtle-ui">{t('secrets.loading')}</p>
        )}
        {modalError != null && (
          <p className="text-sm text-rose-500 mb-2">{modalError}</p>
        )}
        {saveSuccess && (
          <p className="text-sm text-emerald-500 mb-2">{t('secrets.saveSuccess')}</p>
        )}
        {modalValue != null && !editing && (
          <HighlightedValue value={modalValue} />
        )}
        {editing && (
          <div className="flex flex-col gap-3">
            <textarea
              ref={textareaRef}
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              rows={12}
              className="w-full text-xs font-mono bg-slate-800/60 dark:bg-slate-900/60 text-slate-200 p-3 rounded-lg border border-[var(--border)] resize-y focus:outline-none focus:ring-1 focus:ring-[var(--primary)]"
              spellCheck={false}
            />
            <div className="flex items-center justify-end gap-2">
              <button
                onClick={cancelEditing}
                className="btn btn-outline text-xs"
                disabled={saving}
              >
                {t('action.cancel')}
              </button>
              <button
                onClick={() => void handleSave()}
                className="btn btn-primary text-xs"
                disabled={saving}
              >
                {saving ? '...' : t('secrets.saveOverride')}
              </button>
            </div>
          </div>
        )}
      </Modal>
    </>
  );
}
