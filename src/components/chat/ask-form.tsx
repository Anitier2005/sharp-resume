import { useMemo, useState } from "react";
import type { AskQuestion } from "@/lib/agent-actions";
import { Modal } from "@/components/ui/modal";
import { Button, Chip, Input, Select, Textarea } from "@/components/ui";
import styles from "./ask-form.module.css";

/**
 * 会话 agent 发来的填写表单。
 *
 * 提交时把答案拼成一条普通用户消息发回去 —— 后端不需要挂起一个跨请求的
 * async 任务，用户中途切人、关窗口也不会留下悬挂状态。
 * 拼出来的文本刻意保持「人话」：模型下一轮读到的就是一段结构化但不呆板的回答。
 *
 * ESC 与遮罩行为由 `ui/modal` 统一负责：按一次只关这一层，点遮罩不关。
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
    <Modal
      title="补充几个信息"
      subtitle={`${questions.length} 项`}
      onCancel={onCancel}
      bodyClassName={styles.body}
      footer={
        <>
          {/* 报错贴左，按钮贴右：中间靠 auto 撑开，加元素不用重排 */}
          <span className={styles.error}>{error}</span>
          <Button onClick={onCancel}>稍后再说</Button>
          <Button variant="primary" onClick={submit}>
            提交
          </Button>
        </>
      }
    >
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
              <Textarea
                rows={3}
                value={value}
                onChange={(event) => set(question.id, event.target.value)}
              />
            ) : question.kind === "select" ? (
              <Select
                value={value}
                ariaLabel={question.label}
                placeholder="请选择"
                options={(question.options ?? []).map((option) => ({
                  value: option,
                  label: option,
                }))}
                onChange={(next) => set(question.id, next)}
              />
            ) : question.kind === "multi" ? (
              <div className={styles.multi}>
                {(question.options ?? []).map((option) => {
                  const picked = value.split("、").filter(Boolean);
                  const on = picked.includes(option);
                  return (
                    <Chip
                      key={option}
                      pressed={on}
                      onClick={() => {
                        const next = on
                          ? picked.filter((item) => item !== option)
                          : [...picked, option];
                        set(question.id, next.join("、"));
                      }}
                    >
                      {option}
                    </Chip>
                  );
                })}
              </div>
            ) : question.kind === "confirm" ? (
              <div className={styles.multi}>
                {["是", "否"].map((option) => (
                  <Chip
                    key={option}
                    pressed={value === option}
                    onClick={() => set(question.id, option)}
                  >
                    {option}
                  </Chip>
                ))}
              </div>
            ) : (
              <Input
                type={
                  question.kind === "number"
                    ? "number"
                    : question.kind === "date"
                      ? "date"
                      : "text"
                }
                value={value}
                onChange={(event) => set(question.id, event.target.value)}
              />
            )}
          </label>
        );
      })}
    </Modal>
  );
}
