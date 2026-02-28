import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import Modal from './Modal';

interface ConfirmModalProps {
  readonly open: boolean;
  readonly title: string;
  readonly body: ReactNode;
  readonly onConfirm: () => void;
  readonly onCancel: () => void;
  readonly confirmLabel?: string | undefined;
  readonly danger?: boolean | undefined;
}

export default function ConfirmModal({
  open, title, body, onConfirm, onCancel, confirmLabel, danger,
}: ConfirmModalProps) {
  const { t } = useTranslation();
  return (
    <Modal
      open={open}
      title={title}
      onClose={onCancel}
      actions={
        <>
          <button
            onClick={onCancel}
            className="btn btn-outline"
          >
            {t('action.cancel')}
          </button>
          <button
            onClick={onConfirm}
            className={`btn ${
              danger === true
                ? 'btn-danger'
                : 'btn-primary'
            }`}
          >
            {confirmLabel ?? t('action.confirm')}
          </button>
        </>
      }
    >
      {typeof body === 'string' ? <p>{body}</p> : body}
    </Modal>
  );
}
