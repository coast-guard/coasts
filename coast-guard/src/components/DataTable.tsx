import { useCallback, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import EmptyState from './EmptyState';

export interface Column<T> {
  readonly key: string;
  readonly header: string;
  readonly render: (row: T) => ReactNode;
  readonly className?: string | undefined;
  readonly headerClassName?: string | undefined;
}

interface DataTableProps<T> {
  readonly columns: readonly Column<T>[];
  readonly data: readonly T[];
  readonly getRowId: (row: T) => string;
  readonly tableClassName?: string | undefined;
  readonly selectable?: boolean | undefined;
  readonly isRowSelectable?: ((row: T) => boolean) | undefined;
  readonly selectedIds?: ReadonlySet<string> | undefined;
  readonly onSelectionChange?: ((ids: ReadonlySet<string>) => void) | undefined;
  readonly onRowClick?: ((row: T) => void) | undefined;
  readonly emptyMessage?: string | undefined;
}

export default function DataTable<T>({
  columns,
  data,
  getRowId,
  tableClassName,
  selectable,
  isRowSelectable,
  selectedIds,
  onSelectionChange,
  onRowClick,
  emptyMessage,
}: DataTableProps<T>) {
  const { t } = useTranslation();
  const selectableRows = isRowSelectable != null ? data.filter(isRowSelectable) : data;
  const allSelected = selectableRows.length > 0 && selectedIds != null && selectableRows.every((row) => selectedIds.has(getRowId(row)));

  const toggleAll = useCallback(() => {
    if (onSelectionChange == null) return;
    if (allSelected) {
      onSelectionChange(new Set());
    } else {
      onSelectionChange(new Set(selectableRows.map(getRowId)));
    }
  }, [allSelected, selectableRows, getRowId, onSelectionChange]);

  const toggleRow = useCallback(
    (id: string) => {
      if (onSelectionChange == null || selectedIds == null) return;
      const next = new Set(selectedIds);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      onSelectionChange(next);
    },
    [onSelectionChange, selectedIds],
  );

  if (data.length === 0) {
    return <EmptyState message={emptyMessage ?? t('table.empty')} />;
  }

  return (
    <div className="overflow-x-auto">
      <table className={`w-full text-sm ${tableClassName ?? ''}`}>
        <thead>
          <tr className="border-b border-[var(--border)]">
            {selectable === true && (
              <th className="w-10 px-4 py-2.5">
                <input
                  type="checkbox"
                  checked={allSelected}
                  onChange={toggleAll}
                  className="accent-blue-600 rounded-md"
                />
              </th>
            )}
            {columns.map((col) => (
              <th
                key={col.key}
                className={`px-4 py-2.5 text-left text-xs font-semibold text-subtle-ui uppercase tracking-wider ${col.headerClassName ?? ''}`}
              >
                {col.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-[var(--border)]">
          {data.map((row) => {
            const id = getRowId(row);
            const isSelected = selectedIds?.has(id) ?? false;
            const rowSelectable = isRowSelectable == null || isRowSelectable(row);
            return (
              <tr
                key={id}
                className={`transition-colors ${
                  onRowClick != null ? 'cursor-pointer hover:bg-white/35 dark:hover:bg-white/6' : ''
                } ${isSelected ? 'bg-blue-500/10 dark:bg-blue-400/10' : ''}`}
                onClick={() => onRowClick?.(row)}
              >
                {selectable === true && (
                  <td className="w-10 px-4 py-2.5">
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={() => toggleRow(id)}
                      onClick={(e) => e.stopPropagation()}
                      disabled={!rowSelectable}
                      className={`accent-blue-600 rounded-md${!rowSelectable ? ' opacity-30 cursor-not-allowed' : ''}`}
                    />
                  </td>
                )}
                {columns.map((col) => (
                  <td key={col.key} className={`px-4 py-2.5 ${col.className ?? ''}`}>
                    {col.render(row)}
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
