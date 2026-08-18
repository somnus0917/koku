//! 应用根组件：会话管理、登录页与主应用路由。
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ApiError, getAuthSession, logout } from "../api";
import type { AuthSession } from "../types";
import { LoginPage } from "../features/auth/LoginPage";
import { LedgerApp } from "./LedgerApp";

export default function App() {
  const [session, setSession] = useState<AuthSession | null>(null);
  const [checkingSession, setCheckingSession] = useState(true);

  useEffect(() => {
    void getAuthSession()
      .then(setSession)
      .catch((reason) => {
        if (!(reason instanceof ApiError && reason.status === 401)) {
          console.error("Unable to check login session", reason);
        }
        setSession(null);
      })
      .finally(() => setCheckingSession(false));
  }, []);

  useEffect(() => {
    const handleUnauthorized = () => setSession(null);
    window.addEventListener("koku:unauthorized", handleUnauthorized);
    return () => window.removeEventListener("koku:unauthorized", handleUnauthorized);
  }, []);

  if (checkingSession) return <AuthLoadingState />;
  if (!session) return <LoginPage onAuthenticated={setSession} />;
  return (
    <LedgerApp
      username={session.username}
      role={session.role}
      userId={session.id}
      onLogout={async () => {
        try { await logout(); }
        finally { setSession(null); }
      }}
    />
  );
}


function AuthLoadingState() {
  const { t } = useTranslation();
  return <main className="auth-loading"><div className="loading-mark"><span /><span /></div><p>{t("app.checkingSession")}</p></main>;
}

