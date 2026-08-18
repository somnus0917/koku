//! 现金流桑基图：收入来源 → 留存/赤字 → 支出去向。
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { EmptyState } from "../../components/EmptyState";
import { categoryVisual, formatMoney } from "../../lib";
import type { CashFlowSummary } from "../../types";

export interface SankeyDatum {
  id: string;
  name: string;
  amount: number;
  amountText: string;
  color: string;
}

export interface SankeyNode extends SankeyDatum {
  y: number;
  height: number;
}
export function MobileCashFlowGroup({
  label,
  nodes,
  total,
  currency
}: {
  label: string;
  nodes: SankeyNode[];
  total: number;
  currency: string;
}) {
  const { t } = useTranslation();
  return (
    <section className="mobile-flow-group">
      <header><span>{label}</span><small>{t("common.countItems", { count: nodes.length })}</small></header>
      <div className="mobile-flow-list">
        {nodes.map((node) => (
          <article className="mobile-flow-item" key={node.id}>
            <div className="mobile-flow-meta">
              <span><i style={{ background: node.color }} />{node.name}</span>
              <strong>{formatMoney(node.amountText, currency)}</strong>
            </div>
            <div className="mobile-flow-track">
              <i
                style={{
                  width: `${Math.max(4, total > 0 ? (node.amount / total) * 100 : 0)}%`,
                  background: node.color
                }}
              />
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
export function CashFlowSankey({ summary }: { summary: CashFlowSummary }) {
  const { t } = useTranslation();
  const layout = useMemo(() => {
    const retained = Number(summary.retained);
    const sources: SankeyDatum[] = summary.income_sources.map((item) => ({
      id: `income-${item.category_id}`,
      name: item.category_name,
      amount: Number(item.amount),
      amountText: item.amount,
      color: categoryVisual(item.category_name).color
    }));
    const destinations: SankeyDatum[] = summary.expense_destinations.map((item) => ({
      id: `expense-${item.category_id}`,
      name: item.category_name,
      amount: Number(item.amount),
      amountText: item.amount,
      color: categoryVisual(item.category_name).color
    }));
    if (retained < 0) {
      sources.push({
        id: "deficit",
        name: t("insights.cashflow.deficitSource"),
        amount: Math.abs(retained),
        amountText: String(Math.abs(retained)),
        color: "#c27b58"
      });
    } else if (retained > 0) {
      destinations.push({
        id: "retained",
        name: t("insights.cashflow.retained"),
        amount: retained,
        amountText: summary.retained,
        color: "#3f9d70"
      });
    }

    const flowTotal = Number(summary.flow_total);
    const count = Math.max(sources.length, destinations.length, 1);
    const height = Math.max(430, count * 76 + 90);
    const top = 48;
    const bottom = 44;
    const gap = 20;
    const flowArea = height - top - bottom;
    const position = (items: SankeyDatum[]): SankeyNode[] => {
      if (!items.length || flowTotal <= 0) return [];
      const available = Math.max(40, flowArea - gap * Math.max(0, items.length - 1));
      const minimum = Math.min(7, available / items.length);
      const proportional = Math.max(0, available - minimum * items.length);
      let cursor = top;
      return items.map((item) => {
        const nodeHeight = minimum + (item.amount / flowTotal) * proportional;
        const node = { ...item, y: cursor, height: nodeHeight };
        cursor += nodeHeight + gap;
        return node;
      });
    };
    const sourceNodes = position(sources);
    const destinationNodes = position(destinations);
    const sourceHeight = sourceNodes.reduce((sum, item) => sum + item.height, 0);
    const destinationHeight = destinationNodes.reduce((sum, item) => sum + item.height, 0);
    const centerHeight = Math.max(sourceHeight, destinationHeight, 12);
    return {
      height,
      sources: sourceNodes,
      destinations: destinationNodes,
      centerY: (height - centerHeight) / 2,
      centerHeight,
      empty: flowTotal <= 0
    };
  }, [summary, t]);

  if (layout.empty) {
    return (
      <details className="panel cash-flow-panel" open>
        <summary><span><ChevronDown size={18} />{t("insights.cashflow.title")}</span><small>{t("insights.cashflow.subtitle")}</small></summary>
        <EmptyState title={t("insights.cashflow.emptyTitle")} detail={t("insights.cashflow.emptyDetail")} />
      </details>
    );
  }

  let sourceCenterCursor = layout.centerY;
  const sourceRibbons = layout.sources.map((node) => {
    const centerY = sourceCenterCursor;
    sourceCenterCursor += node.height;
    return { node, centerY };
  });
  let destinationCenterCursor = layout.centerY;
  const destinationRibbons = layout.destinations.map((node) => {
    const centerY = destinationCenterCursor;
    destinationCenterCursor += node.height;
    return { node, centerY };
  });

  return (
    <details className="panel cash-flow-panel" open>
      <summary>
        <span><ChevronDown size={18} />{t("insights.cashflow.title")}</span>
      </summary>
      <div className="sankey-scroll">
        <svg
          className="sankey-canvas"
          viewBox={`0 0 1080 ${layout.height}`}
          role="img"
          aria-label={t("insights.cashflow.chartAria", { month: summary.month })}
        >
          <title>{t("insights.cashflow.chartTitle", { month: summary.month })}</title>
          <desc>{t("insights.cashflow.chartDesc")}</desc>
          {sourceRibbons.map(({ node, centerY }) => (
            <path
              key={`source-ribbon-${node.id}`}
              className="sankey-ribbon income-ribbon"
              d={sankeyRibbonPath(158, node.y, node.height, 522, centerY, node.height)}
              style={{ fill: node.color }}
            >
              <title>{t("insights.cashflow.ribbonTitle", { name: node.name, amount: formatMoney(node.amountText, summary.currency) })}</title>
            </path>
          ))}
          {destinationRibbons.map(({ node, centerY }) => (
            <path
              key={`destination-ribbon-${node.id}`}
              className="sankey-ribbon expense-ribbon"
              d={sankeyRibbonPath(546, centerY, node.height, 930, node.y, node.height)}
              style={{ fill: node.color }}
            >
              <title>{t("insights.cashflow.ribbonTitle", { name: node.name, amount: formatMoney(node.amountText, summary.currency) })}</title>
            </path>
          ))}

          {layout.sources.map((node) => (
            <g className="sankey-node" key={node.id}>
              <rect x="144" y={node.y} width="14" height={node.height} rx="5" style={{ fill: node.color }} />
              <text x="132" y={node.y + node.height / 2 - 2} textAnchor="end">
                <tspan className="node-name">{node.name}</tspan>
                <tspan className="node-amount" x="132" dy="17">{formatMoney(node.amountText, summary.currency)}</tspan>
              </text>
            </g>
          ))}

          <g className="sankey-center-node">
            <rect x="522" y={layout.centerY} width="24" height={layout.centerHeight} rx="5" />
            <text x="558" y={layout.centerY + layout.centerHeight / 2 - 3}>
              <tspan className="center-name">{t("insights.cashflow.center")}</tspan>
              <tspan className="center-amount" x="558" dy="20">{formatMoney(summary.flow_total, summary.currency)}</tspan>
            </text>
          </g>

          {layout.destinations.map((node) => (
            <g className="sankey-node" key={node.id}>
              <rect x="930" y={node.y} width="14" height={node.height} rx="5" style={{ fill: node.color }} />
              <text x="958" y={node.y + node.height / 2 - 2}>
                <tspan className="node-name">{node.name}</tspan>
                <tspan className="node-amount" x="958" dy="17">{formatMoney(node.amountText, summary.currency)}</tspan>
              </text>
            </g>
          ))}
        </svg>
      </div>
      <div className="cash-flow-mobile" role="img" aria-label={t("insights.cashflow.mobileAria", { month: summary.month })}>
        <MobileCashFlowGroup
          label={t("common.incomeSources")}
          nodes={layout.sources}
          total={Number(summary.flow_total)}
          currency={summary.currency}
        />
        <div className="mobile-flow-core">
          <span>{t("insights.cashflow.mobileCore")}</span>
          <strong>{formatMoney(summary.flow_total, summary.currency)}</strong>
          <ChevronDown size={17} />
        </div>
        <MobileCashFlowGroup
          label={t("insights.cashflow.mobileDestinations")}
          nodes={layout.destinations}
          total={Number(summary.flow_total)}
          currency={summary.currency}
        />
      </div>
    </details>
  );
}
export function sankeyRibbonPath(
  sourceX: number,
  sourceY: number,
  sourceHeight: number,
  targetX: number,
  targetY: number,
  targetHeight: number
): string {
  const control = (targetX - sourceX) * 0.5;
  return [
    `M ${sourceX} ${sourceY}`,
    `C ${sourceX + control} ${sourceY}, ${targetX - control} ${targetY}, ${targetX} ${targetY}`,
    `L ${targetX} ${targetY + targetHeight}`,
    `C ${targetX - control} ${targetY + targetHeight}, ${sourceX + control} ${sourceY + sourceHeight}, ${sourceX} ${sourceY + sourceHeight}`,
    "Z"
  ].join(" ");
}
