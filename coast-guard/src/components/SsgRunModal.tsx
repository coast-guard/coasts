import { useState, useCallback, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import Modal from './Modal';
import { api } from '../api/endpoints';
import { useSsgState } from '../api/hooks';
import { SpinnerGap, CheckCircle, XCircle } from '@phosphor-icons/react';
import type { BuildProgressEvent } from '../types/api';

type RunPhase = 'confirm' | 'running' | 'done' | 'error';

interface SsgRunModalProps {
    readonly open: boolean;
    readonly project: string;
    readonly onClose: () => void;
    readonly onComplete: () => void;
}

/**
 * Modal that triggers an SSG `run` lifecycle via the
 * `/api/v1/stream/ssg-run` SSE endpoint. Visually mirrors
 * {@link SsgBuildModal} but streams the **runtime** bring-up
 * (outer DinD pull/create + inner compose up) instead of an
 * artifact build. The build_id, project, and service list are
 * pulled from the existing `ssgState` query so the user can
 * confirm what they're about to run before kicking it off.
 */
export default function SsgRunModal({
    open,
    project,
    onClose,
    onComplete,
}: SsgRunModalProps) {
    const { t } = useTranslation();
    const queryClient = useQueryClient();
    const { data: ssgState } = useSsgState(project);

    const [phase, setPhase] = useState<RunPhase>('confirm');
    const [events, setEvents] = useState<BuildProgressEvent[]>([]);
    // Plan is derived from events as they arrive — the daemon's
    // `run_ssg_with_build_id` does not emit an explicit
    // `event.status === 'plan'` frame the way `build_ssg` does
    // (the run path's step count is fixed at 6 internally). We
    // accumulate the unique step names in first-seen order so the
    // checklist renders identically to {@link SsgBuildModal}.
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
        }
    }, [open]);

    const handleRun = useCallback(async () => {
        setPhase('running');
        try {
            const result = await api.ssgRunStreaming(project, (evt) => {
                // Honor an explicit plan frame if the daemon ever
                // starts emitting one (build endpoint does, run
                // endpoint does not as of writing).
                if (evt.status === 'plan' && evt.plan) {
                    setPlan(evt.plan);
                    setTotalSteps(evt.total_steps ?? evt.plan.length);
                    return;
                }
                // Otherwise derive the plan ourselves: append each
                // newly-seen `step` name in the order it first
                // appears. The daemon emits two events per step
                // ("started" and a terminal status); we de-dupe by
                // step name and keep the original ordering.
                if (evt.step != null && evt.step.length > 0) {
                    setPlan((prev) =>
                        prev.includes(evt.step) ? prev : [...prev, evt.step],
                    );
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
                void queryClient.invalidateQueries({ queryKey: ['ssgState', project] });
                setTimeout(() => onComplete(), 1500);
            } else {
                setErrorMsg('Run stream ended unexpectedly without a result.');
                setPhase('error');
            }
        } catch (e) {
            setErrorMsg(e instanceof Error ? e.message : String(e));
            setPhase('error');
        }
    }, [project, queryClient, onComplete]);

    const canClose = phase === 'confirm' || phase === 'done' || phase === 'error';
    const buildId = ssgState?.latest_build_id ?? null;
    const services = ssgState?.services ?? [];

    return (
        <Modal
            open={open}
            wide
            title={
                phase === 'confirm'
                    ? t('ssg.runModalTitle')
                    : phase === 'running'
                        ? t('ssg.running')
                        : phase === 'done'
                            ? t('ssg.runComplete')
                            : t('error.title')
            }
            onClose={canClose ? onClose : () => { }}
            actions={
                phase === 'confirm' ? (
                    <>
                        <button type="button" className="btn btn-outline" onClick={onClose}>
                            {t('action.cancel')}
                        </button>
                        <button
                            type="button"
                            className="btn btn-primary"
                            onClick={() => void handleRun()}
                            disabled={buildId == null}
                        >
                            {t('ssg.startRun')}
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
                    <div className="text-sm text-main">
                        {t('ssg.runHelp', { project })}
                    </div>
                    <div className="text-xs text-subtle-ui space-y-1">
                        <div>
                            {t('build.ssgBuildIdHeader')}:{' '}
                            <span className="font-mono text-main">
                                {buildId ?? t('ssg.runDisabledNoBuild')}
                            </span>
                        </div>
                        {services.length > 0 && (
                            <div>
                                {t('build.ssgServicesHeader')}:{' '}
                                <span className="font-mono text-main">
                                    {services.map((s) => s.name).join(', ')}
                                </span>
                            </div>
                        )}
                    </div>
                </div>
            )}

            {(phase === 'running' || phase === 'done' || phase === 'error') && (
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
                                // Map raw event statuses to one of
                                // four UI states. The run pipeline
                                // emits free-form statuses on
                                // completion (build_id, container_id,
                                // "ready", "{N} loaded"), so we
                                // collapse anything that's not an
                                // explicit failure/in-flight signal
                                // into "ok". Any `fail`/`warn` wins
                                // over a later `ok` if seen.
                                const hasFail = matchingEvents.some(
                                    (e) => e.status === 'fail' || e.status === 'warn',
                                );
                                const lastEvent = matchingEvents[matchingEvents.length - 1];
                                const lastStatus = lastEvent?.status;
                                const uiStatus: 'pending' | 'started' | 'ok' | 'fail' = hasFail
                                    ? 'fail'
                                    : lastStatus == null
                                        ? stepNum < currentStep
                                            ? 'ok'
                                            : stepNum === currentStep
                                                ? 'started'
                                                : 'pending'
                                        : lastStatus === 'started'
                                            ? 'started'
                                            : 'ok';

                                return (
                                    <div
                                        key={step}
                                        className={`flex items-center gap-2 text-xs ${uiStatus === 'pending' ? 'text-subtle-ui' : 'text-main'
                                            }`}
                                    >
                                        {uiStatus === 'started' && (
                                            <SpinnerGap
                                                size={14}
                                                className="animate-spin text-[var(--primary)] shrink-0"
                                            />
                                        )}
                                        {uiStatus === 'ok' && (
                                            <CheckCircle
                                                size={14}
                                                weight="fill"
                                                className="text-emerald-500 shrink-0"
                                            />
                                        )}
                                        {uiStatus === 'fail' && (
                                            <XCircle
                                                size={14}
                                                weight="fill"
                                                className="text-rose-500 shrink-0"
                                            />
                                        )}
                                        {uiStatus === 'pending' && (
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
                                    <div key={i}>{e.detail}</div>
                                ))}
                        </div>
                    )}

                    {phase === 'done' && (
                        <div className="flex items-center gap-2 text-sm text-emerald-600 dark:text-emerald-400">
                            <CheckCircle size={18} weight="fill" />
                            {t('ssg.runComplete')}
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
