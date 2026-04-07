import { useState, useCallback, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import Modal from '../Modal';
import { api } from '../../api/endpoints';
import { useBuildsInspect } from '../../api/hooks';
import { SpinnerGap, CheckCircle, XCircle, WarningCircle } from '@phosphor-icons/react';
import StepIcon from './StepIcon';
import type { BuildProgressEvent } from '../../types/api';

type BuildPhase = 'confirm' | 'building' | 'done' | 'error';

interface BuildModalProps {
  readonly open: boolean;
  readonly project: string;
  readonly onClose: () => void;
  readonly onComplete: () => void;
}

export default function BuildModal({ open, project, onClose, onComplete }: BuildModalProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: inspectData } = useBuildsInspect(project, undefined);
  const [phase, setPhase] = useState<BuildPhase>('confirm');
  const [events, setEvents] = useState<BuildProgressEvent[]>([]);
  const [plan, setPlan] = useState<string[]>([]);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [currentStep, setCurrentStep] = useState<number>(0);
  const [totalSteps, setTotalSteps] = useState<number>(0);
  const [coastfileTypes, setCoastfileTypes] = useState<string[]>([]);
  const [selectedType, setSelectedType] = useState<string>('default');
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
      setSelectedType('default');
    }
  }, [open]);

  useEffect(() => {
    if (open && project) {
      api.buildsCoastfileTypes(project).then((resp) => {
        const all = resp.types ?? [];
        setCoastfileTypes(all.filter((t) => !t.startsWith('remote')));
      }).catch(() => {
        setCoastfileTypes(['default']);
      });
    }
  }, [open, project]);

  const handleBuild = useCallback(async () => {
    const projectRoot = inspectData?.project_root;
    if (!projectRoot) {
      setErrorMsg(t('build.noProjectRoot'));
      setPhase('error');
      return;
    }

    setPhase('building');
    const coastfilePath = selectedType === 'default'
      ? `${projectRoot}/Coastfile`
      : `${projectRoot}/Coastfile.${selectedType}`;

    try {
      const result = await api.buildProject(coastfilePath, false, (evt) => {
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
      });

      if (result.error) {
        setErrorMsg(result.error.error);
        setPhase('error');
      } else if (result.complete) {
        setPhase('done');
        void queryClient.invalidateQueries({ queryKey: ['buildsLs'] });
        void queryClient.invalidateQueries({ queryKey: ['buildsInspect'] });
        setTimeout(() => {
          onComplete();
        }, 1500);
      } else {
        setErrorMsg('Build stream ended unexpectedly without a result.');
        setPhase('error');
      }
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e));
      setPhase('error');
    }
  }, [inspectData, selectedType, t, queryClient, onComplete]);

  const canClose = phase === 'confirm' || phase === 'done' || phase === 'error';

  return (
    <Modal
      open={open}
      wide
      title={phase === 'confirm' ? t('build.newBuildTitle') : phase === 'building' ? t('build.buildingTitle') : phase === 'done' ? t('build.buildComplete') : t('error.title')}
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
        <div className="space-y-3">
          {coastfileTypes.length > 1 && (
            <div className="space-y-1">
              <label className="text-xs font-semibold text-main mb-2 block">{t('build.type')}</label>
              <div className="flex flex-wrap gap-1.5 pt-0.5 pb-0.5">
                {coastfileTypes.map((ct) => (
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
                    {ct}
                  </button>
                ))}
              </div>
              <p className="text-[11px] text-subtle-ui">
                Building from <span className="font-mono">{selectedType === 'default' ? 'Coastfile' : `Coastfile.${selectedType}`}</span>
              </p>
            </div>
          )}
          <div className="flex items-start gap-2 rounded-md border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-950/30 px-3 py-2.5">
            <WarningCircle size={18} weight="fill" className="text-amber-500 shrink-0 mt-0.5" />
            <p className="text-xs text-amber-800 dark:text-amber-300">
              {t('build.rebuildWarning')}
            </p>
          </div>
          <p className="text-xs text-subtle-ui">
            {t('build.rebuildDescription', { project })}
          </p>
        </div>
      )}

      {phase === 'building' && (
        <div className="space-y-3">
          {totalSteps > 0 && (
            <div className="flex items-center gap-2 text-xs text-subtle-ui">
              <SpinnerGap size={14} className="animate-spin text-emerald-500" />
              <span>
                {t('build.stepProgress', { current: currentStep, total: totalSteps })}
              </span>
            </div>
          )}

          {plan.length > 0 && (
            <div className="space-y-1">
              {plan.map((stepName, idx) => {
                const stepNum = idx + 1;
                const stepEvents = events.filter((e) => e.step_number === stepNum || e.step === stepName);
                const lastEvent = stepEvents[stepEvents.length - 1];
                const status = lastEvent?.status ?? (stepNum < currentStep ? 'ok' : stepNum === currentStep ? 'started' : 'pending');
                return (
                  <div key={idx} className="flex items-center gap-2 text-xs">
                    <StepIcon status={status} />
                    <span className={status === 'started' ? 'text-main font-medium' : status === 'ok' ? 'text-green-600 dark:text-green-400' : 'text-subtle-ui'}>
                      [{stepNum}/{totalSteps}] {stepName}
                    </span>
                  </div>
                );
              })}
            </div>
          )}

          <div
            ref={logRef}
            className="max-h-48 overflow-y-auto rounded-md border border-[var(--border)] bg-[var(--code-block-bg)] p-2 font-mono text-[11px] text-[var(--code-block-text)]"
          >
            {events.filter((e) => e.detail != null).map((e, i) => (
              <div key={i} className="flex items-start gap-1.5">
                <StepIcon status={e.status} />
                <span className={
                  e.status === 'ok' ? 'text-green-400' :
                  e.status === 'fail' ? 'text-rose-400' :
                  e.status === 'warn' ? 'text-amber-400' :
                  'text-[var(--code-block-text)] opacity-80'
                }>
                  {e.detail}
                </span>
              </div>
            ))}
            {events.filter((e) => e.detail != null).length === 0 && (
              <span className="text-[var(--code-block-text)] opacity-60">{t('build.waitingForOutput')}</span>
            )}
          </div>
        </div>
      )}

      {phase === 'done' && (
        <div className="flex items-center gap-2 text-sm text-green-600 dark:text-green-400">
          <CheckCircle size={18} weight="fill" />
          <span>{t('build.buildSuccessMessage')}</span>
        </div>
      )}

      {phase === 'error' && errorMsg && (
        <div className="space-y-2">
          <div className="flex items-start gap-2 text-sm text-rose-600 dark:text-rose-400">
            <XCircle size={18} weight="fill" className="shrink-0 mt-0.5" />
            <pre className="whitespace-pre-wrap font-mono text-xs">{errorMsg}</pre>
          </div>
        </div>
      )}
    </Modal>
  );
}
