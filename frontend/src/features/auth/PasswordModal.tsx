//! 修改密码弹窗。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";

export function PasswordModal({
  onClose,
  onSubmit
}: {
  onClose: () => void;
  onSubmit: (oldPassword: string, newPassword: string) => Promise<void>;
}) {  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    if (newPassword.length < 8) {
      setError(t("modals.password.tooShort"));
      setSubmitting(false);
      return;
    }
    if (newPassword !== confirm) {
      setError(t("modals.password.mismatch"));
      setSubmitting(false);
      return;
    }
    try {
      await onSubmit(oldPassword, newPassword);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("modals.password.changeFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="SECURITY" title={t("modals.password.title")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label className="span-two"><span>{t("modals.password.current")}</span><input required type="password" autoFocus autoComplete="current-password" value={oldPassword} onChange={(event) => setOldPassword(event.target.value)} /></label>
          <label className="span-two"><span>{t("modals.password.new")}</span><input required type="password" autoComplete="new-password" value={newPassword} onChange={(event) => setNewPassword(event.target.value)} /></label>
          <label className="span-two"><span>{t("modals.password.confirm")}</span><input required type="password" autoComplete="new-password" value={confirm} onChange={(event) => setConfirm(event.target.value)} /></label>
        </div>
        <p className="fx-hint">{t("modals.password.note")}</p>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting || !oldPassword || !newPassword || !confirm}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.password.submit")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
