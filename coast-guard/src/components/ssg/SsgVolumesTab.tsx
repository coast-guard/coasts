import { useTranslation } from 'react-i18next';
import { useSsgVolumes } from '../../api/hooks';

interface SsgVolumesTabProps {
    readonly project: string;
}

export default function SsgVolumesTab({ project }: SsgVolumesTabProps) {
    const { t } = useTranslation();
    const { data, isLoading, error } = useSsgVolumes(project);

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
    const volumes = data?.volumes ?? [];
    if (volumes.length === 0) {
        return (
            <section className="glass-panel p-6 text-sm text-subtle-ui">
                {t('ssg.noVolumes')}
            </section>
        );
    }

    return (
        <section className="glass-panel overflow-hidden">
            <div className="overflow-x-auto">
                <table className="w-full text-sm">
                    <thead>
                        <tr className="border-b border-[var(--border)] text-left text-xs text-subtle-ui">
                            <th className="px-5 py-2 font-medium">
                                {t('col.name')}
                            </th>
                            <th className="px-4 py-2 font-medium">
                                {t('ssg.col.driver')}
                            </th>
                            <th className="px-4 py-2 font-medium">
                                {t('ssg.col.mountpoint')}
                            </th>
                            <th className="px-4 py-2 font-medium">
                                {t('ssg.col.scope')}
                            </th>
                        </tr>
                    </thead>
                    <tbody className="divide-y divide-[var(--border)]">
                        {volumes.map((v) => (
                            <tr key={v.name}>
                                <td className="px-5 py-2.5 font-mono text-xs text-main">
                                    {v.name}
                                </td>
                                <td className="px-4 py-2.5 font-mono text-xs">
                                    {v.driver}
                                </td>
                                <td className="px-4 py-2.5 font-mono text-xs text-subtle-ui break-all">
                                    {v.mountpoint}
                                </td>
                                <td className="px-4 py-2.5 text-xs text-subtle-ui">
                                    {v.scope}
                                </td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            </div>
        </section>
    );
}
