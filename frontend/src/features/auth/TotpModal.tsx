//! 二步验证（TOTP）管理弹窗：查看状态、开始设置、关闭。
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy, KeyRound, LoaderCircle, LockKeyhole, ShieldCheck } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { getAuthSession, totpDisable, totpEnable, totpSetup } from "../../api";

export function TotpModal({ onClose }: { onClose: () => void }) {
  const [loading, setLoading] = useState(true);
  const [enabled, setEnabled] = useState(false);
  const [step, setStep] = useState<"intro" | "password" | "secret" | "disable">("intro");
  const [secret, setSecret] = useState("");
  const [otpauthUri, setOtpauthUri] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [copied, setCopied] = useState<"" | "secret" | "uri">("");
  const { t } = useTranslation();

  useEffect(() => {
    let cancelled = false;
    getAuthSession()
      .then((session) => {
        if (!cancelled) {
          setEnabled(session.totp_enabled);
          setLoading(false);
        }
      })
      .catch((reason) => {
        if (!cancelled) {
          setError(reason instanceof Error ? reason.message : t("totp.loadFailed"));
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  const copy = async (text: string, which: "secret" | "uri") => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(which);
      window.setTimeout(() => setCopied(""), 1600);
    } catch {
      setError(t("totp.copyFailed"));
    }
  };

  const startSetup = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true); setError(null);
    try {
      const setup = await totpSetup(password);
      setSecret(setup.secret);
      setOtpauthUri(setup.otpauth_uri);
      setPassword("");
      setStep("secret");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("totp.setupFailed"));
    } finally {
      setBusy(false);
    }
  };

  const enable = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true); setError(null);
    try {
      await totpEnable(code.trim());
      setCode("");
      setEnabled(true);
      setNotice(t("totp.enabledNotice"));
      setStep("intro");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("totp.enableFailed"));
    } finally {
      setBusy(false);
    }
  };

  const disable = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true); setError(null);
    try {
      await totpDisable(code.trim());
      setCode("");
      setEnabled(false);
      setNotice(t("totp.disabledNotice"));
      setStep("intro");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("totp.disableFailed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <ModalShell eyebrow="TWO-FACTOR AUTH" title={t("totp.title")} onClose={onClose}>
      <div className="entry-form">
        {loading ? (
          <div className="totp-loading"><LoaderCircle className="spin" size={18} /> {t("totp.loading")}</div>
        ) : step === "secret" ? (
          <>
            <p className="fx-hint">{t("totp.secretIntro")}</p>
            <div className="totp-secret-block">
              <span>{t("totp.secretLabel")}</span>
              <div className="totp-secret-row">
                <code className="totp-secret">{secret}</code>
                <button type="button" className="copy-button" onClick={() => void copy(secret, "secret")}>
                  {copied === "secret" ? <Check size={13} /> : <Copy size={13} />}
                  {copied === "secret" ? t("totp.copied") : t("totp.copy")}
                </button>
              </div>
            </div>
            <div className="totp-secret-block">
              <span>{t("totp.uriLabel")}</span>
              <div className="totp-secret-row">
                <code className="totp-uri">{otpauthUri}</code>
                <button type="button" className="copy-button" onClick={() => void copy(otpauthUri, "uri")}>
                  {copied === "uri" ? <Check size={13} /> : <Copy size={13} />}
                  {copied === "uri" ? t("totp.copied") : t("totp.copy")}
                </button>
              </div>
            </div>
            <form onSubmit={enable}>
              <div className="form-grid">
                <label className="span-two"><span>{t("totp.code")}</span>
                  <input required autoFocus inputMode="numeric" maxLength={6} pattern="[0-9]*" value={code} onChange={(e) => setCode(e.target.value)} placeholder={t("totp.codePlaceholder")} />
                </label>
              </div>
              {error && <div className="form-error">{error}</div>}
              <div className="modal-actions">
                <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
                <button className="primary-button" disabled={busy || code.trim().length !== 6}>
                  {busy && <LoaderCircle className="spin" size={17} />}{t("totp.confirmEnable")}
                </button>
              </div>
            </form>
          </>
        ) : step === "password" ? (
          <form onSubmit={startSetup}>
            <div className="deposit-info"><p>{t("totp.passwordIntro")}</p></div>
            <div className="form-grid">
              <label className="span-two"><span>{t("modals.password.current")}</span>
                <input required type="password" autoFocus autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder={t("totp.passwordPlaceholder")} />
              </label>
            </div>
            {error && <div className="form-error">{error}</div>}
            <div className="modal-actions">
              <button type="button" className="secondary-button" onClick={() => { setError(null); setStep("intro"); }}>{t("totp.back")}</button>
              <button className="primary-button" disabled={busy || !password}>{busy && <LoaderCircle className="spin" size={17} />}{t("totp.next")}</button>
            </div>
          </form>
        ) : enabled ? (
          <div className="totp-enabled">
            <p className="totp-status"><ShieldCheck size={17} /> {t("totp.enabledStatus")}</p>
            {notice && <div className="totp-notice" role="status"><Check size={14} /> {notice}</div>}
            {step === "disable" ? (
              <form onSubmit={disable}>
                <div className="deposit-info"><p>{t("totp.disableIntro")}</p></div>
                <div className="form-grid">
                  <label className="span-two"><span>{t("totp.currentCode")}</span>
                    <input required autoFocus inputMode="numeric" maxLength={6} pattern="[0-9]*" value={code} onChange={(e) => setCode(e.target.value)} placeholder={t("totp.codePlaceholder")} />
                  </label>
                </div>
                {error && <div className="form-error">{error}</div>}
                <div className="modal-actions">
                  <button type="button" className="secondary-button" onClick={() => { setError(null); setCode(""); setStep("intro"); }}>{t("common.cancel")}</button>
                  <button className="primary-button" disabled={busy || code.trim().length !== 6}>{busy && <LoaderCircle className="spin" size={17} />}{t("totp.disable")}</button>
                </div>
              </form>
            ) : (
              <>
                <p className="fx-hint">{t("totp.enabledHint")}</p>
                {error && <div className="form-error">{error}</div>}
                <div className="modal-actions">
                  <button type="button" className="secondary-button" onClick={onClose}>{t("common.close")}</button>
                  <button type="button" className="primary-button" onClick={() => { setError(null); setNotice(null); setStep("disable"); }}><KeyRound size={16} />{t("totp.disable")}</button>
                </div>
              </>
            )}
          </div>
        ) : (
          <>
            <p className="totp-intro-copy"><LockKeyhole size={17} /> {t("totp.disabledStatus")}</p>
            <p className="fx-hint">{t("totp.disabledHint")}</p>
            {notice && <div className="totp-notice" role="status"><Check size={14} /> {notice}</div>}
            {error && <div className="form-error">{error}</div>}
            <div className="modal-actions">
              <button type="button" className="secondary-button" onClick={onClose}>{t("common.close")}</button>
              <button type="button" className="primary-button" onClick={() => { setError(null); setNotice(null); setStep("password"); }}><ShieldCheck size={16} />{t("totp.startSetup")}</button>
            </div>
          </>
        )}
      </div>
    </ModalShell>
  );
}
