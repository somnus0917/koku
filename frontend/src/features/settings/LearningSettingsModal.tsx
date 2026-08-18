//! 个人设置弹窗：自动分类学习数据管理（清除学习数据）。
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, LoaderCircle, Trash2 } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { clearPayeeLearning } from "../../api";

/** 自动分类学习设置：清除自己的学习数据（仅当前用户 ledger）。 */
export function LearningSettingsModal({ onClose }: { onClose: () => void }) {
  const [clearing, setClearing] = useState(false);
  const [cleared, setCleared] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();

  const clearLearning = async () => {
    if (!window.confirm(t("settings.clearLearningConfirm"))) return;
    setClearing(true);
    setError(null);
    try {
      await clearPayeeLearning();
      setCleared(true);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("settings.clearLearningFailed"));
    } finally {
      setClearing(false);
    }
  };

  return (
    <ModalShell eyebrow="LEARNING" title={t("settings.learningTitle")} onClose={onClose}>
      <div className="entry-form">
        <p className="fx-hint">{t("settings.learningHint")}</p>
        {cleared && (
          <div className="totp-notice" role="status">
            <Check size={14} /> {t("settings.clearedLearning")}
          </div>
        )}
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>
            {t("common.close")}
          </button>
          <button
            type="button"
            className="text-button danger"
            onClick={() => void clearLearning()}
            disabled={clearing}
          >
            {clearing ? <LoaderCircle className="spin" size={16} /> : <Trash2 size={16} />}
            {clearing ? t("settings.clearing") : t("settings.clearLearning")}
          </button>
        </div>
      </div>
    </ModalShell>
  );
}
