import { Link } from 'react-router';

export interface BreadcrumbItem {
  readonly label: string;
  readonly to?: string | undefined;
}

export default function Breadcrumb({ items, className }: { readonly items: readonly BreadcrumbItem[]; readonly className?: string }) {
  return (
    <nav className={className ?? 'flex items-center gap-1.5 text-sm text-muted-ui mb-4'}>
      {items.map((item, i) => (
        <span key={i} className="flex items-center gap-1.5">
          {i > 0 && <span className="text-subtle-ui">/</span>}
          {item.to != null ? (
            <Link to={item.to} className="hover:text-main transition-colors">
              {item.label}
            </Link>
          ) : (
            <span className="text-main font-medium">{item.label}</span>
          )}
        </span>
      ))}
    </nav>
  );
}
