import { useTranslation } from 'react-i18next';
import { useSsgState, useSsgBuildInspect } from '../../api/hooks';

interface SsgSecretsTabProps {
    readonly project: string;
}

/**
 * Surfaces the env-var KEYS recorded in the SSG manifest, grouped
 * by service. Mirrors the regular coast Secrets tab's posture: we
 * never leak values, only names. The data comes from the SSG
 * manifest's `services[].env_keys` field, which `coast ssg build`
 * captures from the parsed Coastfile and writes to
 * `manifest.json` (see `coast-ssg/src/build/artifact.rs`).
 */
export default function SsgSecretsTab({ project }: SsgSecretsTabProps) {
    const { t } = useTranslation();
    const { data: state } = useSsgState(project);
    const buildId = state?.latest_build_id ?? '';
    const { data: inspect, isLoading, error } = useSsgBuildInspect(project, buildId);

    if (state?.latest_build_id == null) {
        return (
            <section className="glass-panel p-6 text-sm text-subtle-ui">
                {t('ssg.notBuiltYet')}
            </section>
        );
    }

    if (isLoading) {
        return (
            <section className="glass-panel p-6 text-sm text-subtle-ui">
                Loading…
            </section>
        );
    }
    if (error != null) {
        return (
            <section className="glass-panel p-6 text-sm text-rose-600 dark:text-rose-400">
                {error instanceof Error ? error.message : String(error)}
            </section>
        );
    }

    const services = inspect?.services ?? [];
    const totalKeys = services.reduce(
        (acc, s) => acc + s.env_keys.length,
        0,
    );

    if (totalKeys === 0) {
        return (
            <section className="glass-panel p-6 text-sm text-subtle-ui">
                {t('ssg.noSecrets')}
            </section>
        );
    }

    return (
        <section className="glass-panel p-5 space-y-4">
            <p className="text-xs text-subtle-ui">{t('ssg.secretsHelp')}</p>
            {services.map((svc) => (
                <div key={svc.name}>
                    <h3 className="text-sm font-semibold text-main mb-2 flex items-center gap-2">
                        <span className="font-mono">{svc.name}</span>
                        <span className="text-xs text-subtle-ui font-normal">
                            ({svc.env_keys.length})
                        </span>
                    </h3>
                    {svc.env_keys.length === 0 ? (
                        <p className="text-xs text-subtle-ui">{t('ssg.noSecrets')}</p>
                    ) : (
                        <div className="flex flex-wrap gap-1.5">
                            {svc.env_keys.map((key) => (
                                <span
                                    key={key}
                                    className="px-1.5 py-0.5 rounded bg-amber-500/15 text-amber-700 dark:text-amber-300 font-mono text-[10px]"
                                >
                                    {key}
                                </span>
                            ))}
                        </div>
                    )}
                </div>
            ))}
        </section>
    );
}
