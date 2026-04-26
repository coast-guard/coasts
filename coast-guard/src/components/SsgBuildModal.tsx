import { useState, useCallback, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import Modal from './Modal';
import { api } from '../api/endpoints';
import { useBuildsInspect } from '../api/hooks';
import { SpinnerGap, CheckCircle, XCircle } from '@phosphor-icons/react';
import type { BuildProgressEvent } from '../types/api';

type BuildPhase = 'confirm' | 'building' | 'done' | 'error';

interface SsgBuildModalProps {
    readonly open: boolean;
    readonly project: string;
    readonly onClose: () => void;
    readonly onComplete: () => void;
}

/**
 * Modal that triggers a Shared Service Group build via the
 * `/api/v1/stream/ssg-build` SSE endpoint. Visually mirrors
 * `RemoteBuildModal` but with no remote selection and no Coastfile
 * variant picker — SSGs always build `Coastfile.shared_service_groups`
 * for the project.
 */
export default function SsgBuildModal({
    open,
    project,
    onClose,
    onComplete,
}: SsgBuildModalProps) {
    const { t } = useTranslation();
    const queryClient = useQueryClient();
    const { data: inspectData } = useBuildsInspect(project, undefined);

    const [phase, setPhase] = useState<BuildPhase>('confirm');
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
        }
    }, [open]);

    const handleBuild = useCallback(async () => {
        setPhase('building');
        try {
            const result = await api.ssgBuild(
                project,
                inspectData?.project_root ?? undefined,
                undefined,
                undefined,
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
                void queryClient.invalidateQueries({ queryKey: ['ssgBuilds', project] });
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
    }, [project, inspectData, queryClient, onComplete]);

    const canClose = phase === 'confirm' || phase === 'done' || phase === 'error';

    return (
        <Modal
            open={open}
            wide
            title={
                phase === 'confirm'
                    ? t('build.ssgBuildModalTitle')
                    : phase === 'building'
                        ? t('build.building')
                        : phase === 'done'
                            ? t('build.buildComplete')
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
                    <div className="text-sm text-main">
                        {t('build.ssgBuildHelp', { project })}
                    </div>
                    <div className="text-xs text-subtle-ui">
                        Coastfile:{' '}
                        <span className="font-mono text-main">
                            Coastfile.shared_service_groups
                        </span>
                    </div>
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
                                        className={`flex items-center gap-2 text-xs ${status === 'pending' ? 'text-subtle-ui' : 'text-main'
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
                                    <div key={i}>{e.detail}</div>
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
