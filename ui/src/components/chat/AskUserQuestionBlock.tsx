import type { PendingQuestion } from "../../types/messages";

export function AskUserQuestionBlock({
  pendingQuestion,
  questionSelections,
  questionCustomText,
  questionError,
  isStreaming,
  toggleQuestionOption,
  setQuestionCustomText,
  answerQuestion,
}: {
  pendingQuestion: PendingQuestion;
  questionSelections: Record<string, string[]>;
  questionCustomText: Record<string, string>;
  questionError: string | null;
  isStreaming: boolean;
  toggleQuestionOption: (qid: string, oid: string, multi?: boolean) => void;
  setQuestionCustomText: (value: Record<string, string>) => void;
  answerQuestion: () => Promise<void>;
}) {
  const allAnswered = pendingQuestion.questions.every((q) => {
    if (questionSelections[q.id]?.length) return true;
    if (q.allowCustom && questionCustomText[q.id]?.trim()) return true;
    return false;
  });
  return (
    <article className="ask-user-question">
      <header>Agent 需要你的选择</header>
      {questionError && <p className="question-error">{questionError}</p>}
      {pendingQuestion.questions.map((q) => (
        <div key={q.id} className="question-block">
          <p className="question-prompt">{q.prompt}</p>
          <div className="question-options">
            {q.options.map((opt) => {
              const selected = questionSelections[q.id]?.includes(opt.id);
              return (
                <button
                  key={opt.id}
                  type="button"
                  className={selected ? "option selected" : "option"}
                  onClick={() => toggleQuestionOption(q.id, opt.id, q.allowMultiple)}
                >
                  {opt.label}
                </button>
              );
            })}
          </div>
          {q.allowCustom && (
            <input
              type="text"
              className="question-custom-input"
              placeholder="或输入自定义内容..."
              value={questionCustomText[q.id] ?? ""}
              onChange={(e) =>
                setQuestionCustomText({
                  ...questionCustomText,
                  [q.id]: e.target.value,
                })
              }
              disabled={isStreaming}
            />
          )}
        </div>
      ))}
      <button
        type="button"
        className="btn-confirm"
        disabled={!allAnswered || isStreaming}
        onClick={() => void answerQuestion()}
      >
        确认选择
      </button>
    </article>
  );
}
