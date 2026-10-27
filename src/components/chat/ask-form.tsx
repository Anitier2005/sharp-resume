import { useEffect, useMemo, useState } from "react";
import type { AskQuestion } from "@/lib/agent-actions";
import shared from "@/components/ui/shared.module.css";
import styles from "./ask-form.module.css";

/**
 * 会话 agent 发来的填写表单。
 *
 * 提交时把答案拼成一条普通用户消息发回去 —— 后端不需要挂起一个跨请求的
 * async 任务，用户中途切人、关窗口也不会留下悬挂状态。
 * 拼出来的文本刻意保持「人话」：模型下一轮读到的就是一段结构化但不呆板的回答。
 */
export function AskForm({
  questions,
  onSubmit,
  onCancel,
}: {
  questions: AskQuestion[];
  onSubmit: (text: string) => void;
  onCancel: () => void;
}) {
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [error, setError] = useState("");

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const missing = useMemo(
    () =>
      questions.filter(
        (question) => question.required && !(answers[question.id] ?? "").trim(),
      ),
    [questions, answers],
  );

  const set = (id: string, value: string) =>
    setAnswers((prev) => ({ ...prev, [id]: value }));

  const submit = () => {
    if (missing.length) {
      setError(`还有 ${missing.length} 项必填没填`);
      return;
    }
    const lines = questions.map((question, index) => {
      const value = (answers[question.id] ?? "").trim();
      return `${index + 1}. ${question.label}：${value || "（未填）"}`;
    });
    onSubmit(`（表单回答）\n${lines.join("\n")}`);
  };

  return (
    <div
      className={shared.veil}
      data-print="hide"
      onMouseDown={(event) => event.target === event.currentTarget && onCancel()}
    >
      <div className={styles.box} role="dialog" aria-modal="true" aria-label="补充信息">
        <div className={styles.head}>
          <span className={styles.title}>补充几个信息</span>
          <span className={styles.count}>{questions.length} 项</span>
        </div>

        <div className={styles.body}>
          {questions.map((question) => {
            const value = answers[question.id] ?? "";
            return (
              <label key={question.id} className={styles.row}>
                <span className={styles.label}>
                  {question.label}
                  {question.required && <i className={styles.req}>*</i>}
                </span>
                {question.hint && <span className={styles.hint}>{question.hint}</span>}

                {question.kind === "textarea" ? (
                  <textarea
                    className={styles.input}
                    rows={3}
                    value={value}
                    onChange={(event) => set(question.id, event.target.value)}
                  />
                ) : question.kind === "select" ? (
                  <select
                    className={styles.input}
                    value={value}
                    onChange={(event) => set(question.id, event.target.value)}
                  >
                    <option value="">请选择</option>
                    {(question.options ?? []).map((option) => (
                      <option key={option} value={option}>
                        {option}
                      </option>
                    ))}
                  </select>
                ) : question.kind === "multi" ? (
                  <div className={styles.multi}>
                    {(question.options ?? []).map((option) => {
                      const picked = value.split("、").filter(Boolean);
                      const on = picked.includes(option);
                      return (
                        <button
                          key={option}
                          type="button"
                          className={on ? `${styles.chip} ${styles.chipOn}` : styles.chip}
                          aria-pressed={on}
                          onClick={() => {
                            const next = on
                              ? picked.filter((item) => item !== option)
                              : [...picked, option];
                            set(question.id, next.join("、"));
                          }}
                        >
                          {option}
                        </button>
                      );
                    })}
                  </div>
                ) : question.kind === "confirm" ? (
                  <div className={styles.multi}>
                    {["是", "否"].map((option) => (
                      <button
                        key={option}
                        type="button"
                        className={value === option ? `${styles.chip} ${styles.chipOn}` : styles.chip}
                        aria-pressed={value === option}
                        onClick={() => set(question.id, option)}
                      >
                        {option}
                      </button>
                    ))}
                  </div>
                ) : (
                  <input
                    className={styles.input}
                    type={question.kind === "number" ? "number" : question.kind === "date" ? "date" : "text"}
                    value={value}
                    onChange={(event) => set(question.id, event.target.value)}
                  />
                )}
              </label>
            );
          })}
        </div>

        <div className={styles.foot}>
          <span className={styles.error}>{error}</span>
          <button type="button" className={styles.ghost} onClick={onCancel}>
            稍后再说
          </button>
          <button type="button" className={styles.primary} onClick={submit}>
            提交
          </button>
        </div>
      </div>
    </div>
  );
}
