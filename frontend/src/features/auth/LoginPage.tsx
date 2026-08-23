//! 登录页：账号密码 + TOTP 二阶段登录。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff, Globe, LoaderCircle, LockKeyhole, Monitor, Moon, ShieldCheck, Sun } from "lucide-react";
import { changeLanguage } from "../../i18n";
import { useTheme } from "../../theme";
import { ApiError, login, verifyTotp } from "../../api";
import type { AuthSession } from "../../types";

export function LoginPage({ onAuthenticated }: { onAuthenticated: (session: AuthSession) => void }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [step, setStep] = useState<"credentials" | "totp">("credentials");
  const [totpToken, setTotpToken] = useState("");
  const [code, setCode] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { theme, setTheme } = useTheme();
  const { t, i18n } = useTranslation();

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      const result = await login(email, password);
      // 已启用二步验证：先拿 totp_token，切到动态码步骤再完成登录。
      if ("totp_required" in result) {
        setTotpToken(result.totp_token);
        setCode("");
        setStep("totp");
        setSubmitting(false);
        return;
      }
      onAuthenticated(result);
    } catch (reason) {
      setError(reason instanceof ApiError && reason.status === 401 ? t("login.invalidCredentials") : reason instanceof Error ? reason.message : t("login.unavailable"));
      setSubmitting(false);
    }
  };

  const submitTotp = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      onAuthenticated(await verifyTotp(totpToken, code.trim()));
    } catch (reason) {
      setError(reason instanceof ApiError && reason.status === 401 ? t("login.totpInvalid") : reason instanceof Error ? reason.message : t("login.unavailable"));
      setSubmitting(false);
    }
  };

  return (
    <main className="login-page">
      <div className="login-corner-actions">
        <button
          className="login-theme-button"
          type="button"
          onClick={() => setTheme(theme === "light" ? "dark" : theme === "dark" ? "system" : "light")}
          aria-label={t("common.themeToggle")}
          title={theme === "light" ? t("common.themeLight") : theme === "dark" ? t("common.themeDark") : t("common.themeSystem")}
        >
          {theme === "light" ? <Sun size={18} /> : theme === "dark" ? <Moon size={18} /> : <Monitor size={18} />}
        </button>
        <button
          className="login-theme-button"
          type="button"
          onClick={() => void changeLanguage(i18n.language?.toLowerCase().startsWith("en") ? "zh" : "en")}
          aria-label={t("common.language")}
          title={t("common.language")}
        >
          <Globe size={18} />
        </button>
      </div>
      <section className="login-story" aria-label={t("login.storyLabel")}>
        <div className="login-brand"><div className="brand-mark" aria-hidden="true"><span /><span /></div><div><strong>Koku</strong><small>PRIVATE LEDGER</small></div></div>
        <div className="login-story-copy">
          <span>YOUR MONEY, QUIETLY KEPT</span>
          <h1>{t("login.headlineLine1")}<br />{t("login.headlineLine2")}</h1>
          <p>{t("login.blurb")}</p>
        </div>
        <div className="login-trust-row"><span><ShieldCheck size={16} />{t("login.selfHosted")}</span><span><LockKeyhole size={16} />{t("login.encryptedSession")}</span></div>
      </section>
      <section className="login-panel">
        {step === "totp" ? (
          <form className="login-card" onSubmit={submitTotp}>
            <div className="login-lock"><ShieldCheck size={20} /></div>
            <span className="login-eyebrow">TWO-FACTOR AUTH</span>
            <h2>{t("totp.title")}</h2>
            <p>{t("login.totpIntro")}</p>
            <label><span>{t("login.totpCode")}</span><input autoFocus required inputMode="numeric" maxLength={6} pattern="[0-9]*" autoComplete="one-time-code" value={code} onChange={(event) => setCode(event.target.value)} placeholder={t("totp.codePlaceholder")} /></label>
            {error && <div className="login-error" role="alert">{error}</div>}
            <button className="login-submit" disabled={submitting || code.trim().length !== 6}>{submitting ? <LoaderCircle className="spin" size={18} /> : <ShieldCheck size={17} />}{submitting ? t("login.verifying") : t("login.verifyAndLogin")}</button>
            <button type="button" className="login-back" onClick={() => { setStep("credentials"); setError(null); setCode(""); }}>{t("login.backToCredentials")}</button>
          </form>
        ) : (
          <form className="login-card" onSubmit={submit}>
            <div className="login-lock"><LockKeyhole size={20} /></div>
            <span className="login-eyebrow">WELCOME BACK</span>
            <h2>{t("login.title")}</h2>
            <p>{t("login.subtitle")}</p>
            <label><span>{t("login.email")}</span><input autoFocus required type="email" autoComplete="email" value={email} onChange={(event) => setEmail(event.target.value)} placeholder={t("login.emailPlaceholder")} /></label>
            <label><span>{t("login.password")}</span><div className="password-field"><input required type={showPassword ? "text" : "password"} autoComplete="current-password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder={t("login.passwordPlaceholder")} /><button type="button" onClick={() => setShowPassword((value) => !value)} aria-label={showPassword ? t("login.hidePassword") : t("login.showPassword")}>{showPassword ? <EyeOff size={17} /> : <Eye size={17} />}</button></div></label>
            {error && <div className="login-error" role="alert">{error}</div>}
            <button className="login-submit" disabled={submitting || !email || !password}>{submitting ? <LoaderCircle className="spin" size={18} /> : <LockKeyhole size={17} />}{submitting ? t("login.verifying") : t("login.signIn")}</button>
            <small className="login-footnote">{t("login.footnote")}</small>
          </form>
        )}
      </section>
    </main>
  );
}
