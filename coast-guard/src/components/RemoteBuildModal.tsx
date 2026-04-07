import { useState, useCallback, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import Modal from './Modal';
import { api } from '../api/endpoints';
import { useRemotesLs, useBuildsInspect } from '../api/hooks';
import { SpinnerGap, CheckCircle, XCircle } from '@phosphor-icons/react';
import type { BuildProgressEvent } from '../types/api';

type BuildPhase = 'confirm' | 'building' | 'done' | 'error';

interface RemoteBuildModalProps {
  readonly open: boolean;
  readonly project: string;
  readonly onClose: () => void;
  readonly onComplete: () => void;
}

export default function RemoteBuildModal({
  open,
  project,
  onClose,
  onComplete,
}: RemoteBuildModalProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: remotesData } = useRemotesLs();
  const { data: inspectData } = useBuildsInspect(project, undefined);
  const remotes = remotesData?.remotes ?? [];

  const [phase, setPhase] = useState<BuildPhase>('confirm');
  const [selectedRemote, setSelectedRemote] = useState('');
  const [remoteArch, setRemoteArch] = useState<string | null>(null);
  const [remoteTypes, setRemoteTypes] = useState<string[]>([]);
  const [selectedType, setSelectedType] = useState('remote');
  const [events, setEvents] = useState<BuildProgressEvent[]>([]);
  const [plan, setPlan] = useState<string[]>([]);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [currentStep, setCurrentStep] = useState(0);
  const [totalSteps, setTotalSteps] = useState(0);
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [events]);

  useEffect(() => {
    if (!open) {
      setPhase('confirm');
      setEvents([]);
      setPlan([]);
      setErrorMsg(null);
      setCurrentStep(0);
      setTotalSteps(0);
      setSelectedRemote('');
      setRemoteArch(null);
      setRemoteTypes([]);
      setSelectedType('remote');
    }
  }, [open]);

  useEffect(() => {
    if (open && project) {
      api
        .buildsCoastfileTypes(project)
        .then((resp) => {
          const all = resp.types ?? [];
          const filtered = all.filter((t) => t.startsWith('remote'));
          filtered.sort((a, b) => {
            if (a === 'remote') return -1;
            if (b === 'remote') return 1;
            return a.localeCompare(b);
          });
          setRemoteTypes(filtered.length > 0 ? filtered : ['remote']);
          if (filtered.length > 0 && filtered[0]) {
            setSelectedType(filtered[0]);
          }
        })
        .catch(() => {
          setRemoteTypes(['remote']);
        });
    }
  }, [open, project]);

  useEffect(() => {
    if (open && remotes.length === 1 && remotes[0] && !selectedRemote) {
      setSelectedRemote(remotes[0].name);
    }
  }, [open, remotes, selectedRemote]);

  useEffect(() => {
    if (!selectedRemote) {
      setRemoteArch(null);
      return;
    }
    setRemoteArch(null);
    api
      .remoteArch(selectedRemote)
      .then((resp) => setRemoteArch(resp.arch))
      .catch(() => setRemoteArch('unknown'));
  }, [selectedRemote]);

  const handleBuild = useCallback(async () => {
    if (!selectedRemote) return;

    const projectRoot = inspectData?.project_root;
    if (!projectRoot) {
      setErrorMsg(t('build.noProjectRoot'));
      setPhase('error');
      return;
    }

    setPhase('building');
    const coastfilePath = `${projectRoot}/Coastfile.${selectedType}`;

    try {
      const result = await api.remoteBuild(
        selectedRemote,
        coastfilePath,
        false,
        (evt) => {
          if (evt.status === 'plan' && evt.plan) {
            setPlan(evt.plan);
            setTotalSteps(evt.total_steps ?? evt.plan.length);
            return;
          }
          if (evt.step_number != null) {
            setCurrentStep(evt.step_number);
          }
          if (evt.total_steps != null) {
            setTotalSteps(evt.total_steps);
          }
          setEvents((prev) => [...prev, evt]);
        },
      );

      if (result.error) {
        setErrorMsg(result.error.error);
        setPhase('error');
      } else if (result.complete) {
        setPhase('done');
        void queryClient.invalidateQueries({ queryKey: ['buildsLs'] });
        setTimeout(() => onComplete(), 1500);
      } else {
        setErrorMsg('Build stream ended unexpectedly without a result.');
        setPhase('error');
      }
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e));
      setPhase('error');
    }
  }, [selectedRemote, selectedType, inspectData, t, queryClient, onComplete]);

  const canClose = phase === 'confirm' || phase === 'done' || phase === 'error';

  return (
    <Modal
      open={open}
      wide
      title={
        phase === 'confirm'
          ? t('build.remoteBuildTitle')
          : phase === 'building'
            ? t('build.building')
            : phase === 'done'
              ? t('build.buildComplete')
              : t('error.title')
      }
      onClose={canClose ? onClose : () => {}}
      actions={
        phase === 'confirm' ? (
          <>
            <button type="button" className="btn btn-outline" onClick={onClose}>
              {t('action.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              disabled={!selectedRemote}
              onClick={() => void handleBuild()}
            >
              {t('build.startBuild')}
            </button>
          </>
        ) : phase === 'done' ? (
          <button type="button" className="btn btn-outline" onClick={onComplete}>
            {t('action.close')}
          </button>
        ) : phase === 'error' ? (
          <button type="button" className="btn btn-outline" onClick={onClose}>
            {t('action.close')}
          </button>
        ) : undefined
      }
    >
      {phase === 'confirm' && (
        <div className="space-y-4">
          <div>
            <label className="text-xs font-semibold text-main mb-2 block">
              {t('build.selectRemote')}
            </label>
            <div className="flex flex-wrap gap-1.5">
              {remotes.map((r) => (
                <button
                  key={r.name}
                  type="button"
                  onClick={() => setSelectedRemote(r.name)}
                  className={`px-3 py-1.5 rounded-md text-xs font-mono border transition-colors ${
                    selectedRemote === r.name
                      ? 'bg-[var(--primary)] border-[var(--primary-strong)] text-white'
                      : 'bg-[var(--surface-muted)] border-[var(--border)] text-main hover:bg-[var(--surface-strong)]'
                  }`}
                >
                  {r.name}
                </button>
              ))}
            </div>
          </div>

          {selectedRemote && (
            <div className="flex items-center gap-3 text-xs text-subtle-ui">
              <span>
                {remotes.find((r) => r.name === selectedRemote)?.user}@
                {remotes.find((r) => r.name === selectedRemote)?.host}:
                {remotes.find((r) => r.name === selectedRemote)?.port}
              </span>
              {remoteArch && (
                <>
                  <span className="text-[var(--border)]">|</span>
                  <span>
                    {t('build.remoteArch')}:{' '}
                    <strong className="text-main font-mono">{remoteArch}</strong>
                  </span>
                </>
              )}
              {!remoteArch && (
                <SpinnerGap size={14} className="animate-spin text-subtle-ui" />
              )}
            </div>
          )}

          <div>
            <label className="text-xs font-semibold text-main mb-2 block">
              Coastfile
            </label>
            <div className="flex flex-wrap gap-1.5 pt-0.5 pb-0.5">
              {remoteTypes.map((ct) => (
                <button
                  key={ct}
                  type="button"
                  onClick={() => setSelectedType(ct)}
                  className={`px-2.5 py-1 rounded-md text-[11px] font-mono border cursor-pointer transition-colors ${
                    selectedType === ct
                      ? 'bg-[var(--primary)] border-[var(--primary-strong)] text-white'
                      : 'bg-[var(--surface-muted)] border-[var(--border)] text-main hover:bg-[var(--surface-strong)]'
                  }`}
                >
                  Coastfile.{ct}
                </button>
              ))}
            </div>
          </div>

          <p className="text-xs text-subtle-ui">
            {t('build.rebuildDescription', { project })}
          </p>
        </div>
      )}

      {(phase === 'building' || phase === 'done' || phase === 'error') && (
        <div className="space-y-3">
          {totalSteps > 0 && (
            <div className="text-xs text-subtle-ui">
              Step {currentStep} / {totalSteps}
            </div>
          )}

          {plan.length > 0 && (
            <div className="space-y-1">
              {plan.map((step, i) => {
                const stepNum = i + 1;
                const matchingEvents = events.filter(
                  (e) => e.step === step || e.step_number === stepNum,
                );
                const lastEvent = matchingEvents[matchingEvents.length - 1];
                const status =
                  lastEvent?.status ??
                  (stepNum < currentStep
                    ? 'ok'
                    : stepNum === currentStep
                      ? 'started'
                      : 'pending');

                return (
                  <div
                    key={step}
                    className={`flex items-center gap-2 text-xs ${
                      status === 'pending' ? 'text-subtle-ui' : 'text-main'
                    }`}
                  >
                    {status === 'started' && (
                      <SpinnerGap
                        size={14}
                        className="animate-spin text-[var(--primary)] shrink-0"
                      />
                    )}
                    {status === 'ok' && (
                      <CheckCircle
                        size={14}
                        weight="fill"
                        className="text-emerald-500 shrink-0"
                      />
                    )}
                    {status === 'fail' && (
                      <XCircle
                        size={14}
                        weight="fill"
                        className="text-rose-500 shrink-0"
                      />
                    )}
                    {status === 'pending' && (
                      <span className="w-3.5 h-3.5 rounded-full border border-[var(--border)] shrink-0" />
                    )}
                    <span>{step}</span>
                  </div>
                );
              })}
            </div>
          )}

          {events.filter((e) => e.detail != null).length > 0 && (
            <div
              ref={logRef}
              className="max-h-40 overflow-auto text-[11px] font-mono text-subtle-ui bg-[var(--surface-muted)] rounded-md p-2 space-y-0.5"
            >
              {events
                .filter((e) => e.detail != null)
                .map((e, i) => (
                  <div key={i}>
                    {e.detail}
                  </div>
                ))}
            </div>
          )}

          {phase === 'done' && (
            <div className="flex items-center gap-2 text-sm text-emerald-600 dark:text-emerald-400">
              <CheckCircle size={18} weight="fill" />
              {t('build.buildComplete')}
            </div>
          )}

          {phase === 'error' && errorMsg && (
            <div className="p-3 rounded-md bg-rose-500/10 text-sm text-rose-600 dark:text-rose-400">
              {errorMsg}
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}
