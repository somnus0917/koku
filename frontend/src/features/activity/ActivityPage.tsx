//! 账本活动轨迹：展示已完成操作的简明、不可编辑记录。
import { useEffect, useState } from "react";
import { History, RefreshCcw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { loadActivity } from "../../api";
import { EmptyState } from "../../components/EmptyState";
import { PageTitle } from "../../components/PageTitle";
import { formatDate } from "../../lib";
import type { ActivityEvent } from "../../types";

export function ActivityPage() {
  const { t } = useTranslation();
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const load = async () => {
    setLoading(true);
    try { setEvents(await loadActivity()); setError(null); }
    catch (reason) { setError(reason instanceof Error ? reason.message : t("activity.loadFailed")); }
    finally { setLoading(false); }
  };
  useEffect(() => { void load(); }, []);
  return <div className="page page-enter">
    <PageTitle eyebrow="ACTIVITY HISTORY" title={t("activity.title")} actions={<button className="text-button" onClick={() => void load()} disabled={loading}><RefreshCcw className={loading ? "spin" : ""} size={16} /> {t("common.refresh")}</button>} />
    <p className="page-hint">{t("activity.hint")}</p>
    {error ? <div className="inline-error">{error}</div> : events.length === 0 && !loading ? <EmptyState title={t("activity.emptyTitle")} detail={t("activity.emptyDetail")} /> : <section className="activity-feed panel" aria-busy={loading}>
      {events.map((event) => <article key={event.id}><span><History size={15} /></span><div><strong>{event.summary}</strong><small>{t(`activity.action.${event.action}`, { defaultValue: event.action })} · {formatDate(event.occurred_at)}</small></div></article>)}
      {loading && <p>{t("common.loading")}</p>}
    </section>}
  </div>;
}
