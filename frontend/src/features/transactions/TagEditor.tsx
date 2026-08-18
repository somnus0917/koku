//! 交易标签编辑器：回车添加、点击 × 移除，附带已有标签建议。
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";

/** 标签编辑：回车添加、点击 × 移除，附带已有标签建议。 */
export function TagEditor({
  value,
  onChange,
  suggestions
}: {
  value: string[];
  onChange: (tags: string[]) => void;
  suggestions: string[];
}) {
  const [draft, setDraft] = useState("");
  const { t } = useTranslation();
  const add = () => {
    const name = draft.trim();
    if (name && !value.includes(name)) onChange([...value, name]);
    setDraft("");
  };
  return (
    <div className="tag-editor">
      {value.map((name) => (
        <span className="tag-chip" key={name}>
          {name}
          <button type="button" onClick={() => onChange(value.filter((item) => item !== name))} aria-label={t("modals.tagEditor.removeAria", { name })}><X size={11} /></button>
        </span>
      ))}
      <input
        list="koku-tag-suggestions"
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            add();
          }
        }}
        onBlur={add}
        placeholder={t("modals.tagEditor.placeholder")}
      />
      <datalist id="koku-tag-suggestions">
        {suggestions.map((name) => <option key={name} value={name} />)}
      </datalist>
    </div>
  );
}
