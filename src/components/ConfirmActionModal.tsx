import type { ToolConfirmRequest } from "../lib/tauri";

type ConfirmActionModalProps = {
  request: ToolConfirmRequest | null;
  onRespond: (approved: boolean) => void;
};

export function ConfirmActionModal({ request, onRespond }: ConfirmActionModalProps) {
  if (!request) return null;

  return (
    <div className="confirm-backdrop" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
      <div className="confirm-modal">
        <h2 id="confirm-title" className="confirm-modal__title">
          {request.title}
        </h2>
        <p className="confirm-modal__detail">{request.detail}</p>
        <div className="confirm-modal__actions">
          <button
            type="button"
            className="confirm-btn confirm-btn--cancel"
            onClick={() => onRespond(false)}
          >
            বাতিল করুন
          </button>
          <button
            type="button"
            className="confirm-btn confirm-btn--approve"
            onClick={() => onRespond(true)}
          >
            অনুমতি দিন
          </button>
        </div>
      </div>
    </div>
  );
}
