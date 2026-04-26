import { useTranslation } from 'react-i18next';
import { useSsgImages } from '../../api/hooks';

interface SsgImagesTabProps {
    readonly project: string;
}

/**
 * Surfaces the inner Docker daemon's images (i.e., what `docker
 * image ls` shows from inside the SSG's outer DinD: postgres:15,
 * redis:7, etc.). The daemon execs `docker image ls --format
 * '{{json .}}'` inside the DinD and parses the line-delimited
 * output.
 */
export default function SsgImagesTab({ project }: SsgImagesTabProps) {
    const { t } = useTranslation();
    const { data, isLoading, error } = useSsgImages(project);

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
    const images = data?.images ?? [];
    if (images.length === 0) {
        return (
            <section className="glass-panel p-6 text-sm text-subtle-ui">
                {t('ssg.noImages')}
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
                                {t('build.repository')}
                            </th>
                            <th className="px-4 py-2 font-medium">{t('build.tag')}</th>
                            <th className="px-4 py-2 font-medium">{t('build.imageId')}</th>
                            <th className="px-4 py-2 font-medium">{t('build.size')}</th>
                            <th className="px-4 py-2 font-medium">{t('build.created')}</th>
                        </tr>
                    </thead>
                    <tbody className="divide-y divide-[var(--border)]">
                        {images.map((img) => (
                            <tr key={`${img.repository}:${img.tag}:${img.id}`}>
                                <td className="px-5 py-2.5 font-mono text-xs text-[var(--primary)]">
                                    {img.repository}
                                </td>
                                <td className="px-4 py-2.5 font-mono text-xs">
                                    {img.tag}
                                </td>
                                <td className="px-4 py-2.5 font-mono text-xs text-subtle-ui">
                                    {img.id}
                                </td>
                                <td className="px-4 py-2.5 text-xs text-subtle-ui">
                                    {img.size}
                                </td>
                                <td className="px-4 py-2.5 text-xs text-subtle-ui">
                                    {img.created}
                                </td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            </div>
        </section>
    );
}
