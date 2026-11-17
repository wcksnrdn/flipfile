import { cls } from "../../shared/cls";
import { writingField, type Answer, type Summary } from "../summary";
import { CheckIcon, Sparkle } from "./icons";
import { Caret, Peek, Rich, Section, Skeleton } from "./primitives";

type Props = {
  summary: Summary;
  writing: boolean;
  peek?: string[];
  checked: ReadonlySet<number>;
  onCheck: (index: number) => void;
  answers: Answer[];
  /** Null while a question can't be asked (still writing, or another answer is on its way) */
  onAsk: ((question: string) => void) | null;
};

const filled = (items: string[] | undefined) => (items ?? []).filter((s) => s.trim());

/** The summary as it streams in: each part appears as soon as the model starts on it */
export default function SummaryView({ summary, writing, checked, onCheck, answers, onAsk, peek }: Props) {
  const field = writing ? writingField(summary) : null;
  const points = filled(summary.points);
  const facts = (summary.facts ?? []).filter((f) => f.value?.trim() || f.label?.trim());
  const actions = filled(summary.actions);
  const keywords = filled(summary.keywords);
  const questions = filled(summary.questions);
  const started = (key: keyof Summary) => summary[key] !== undefined;

  return (
    <article className="sp-summary">
      {summary.type ? (
        <div className="sp-type">
          <Sparkle />
          {summary.type}
        </div>
      ) : (
        <Skeleton lines={1} className="eyebrow" />
      )}
      {summary.title ? (
        <h1 className="sp-title">
          {summary.title}
          <Caret on={field === "title"} />
        </h1>
      ) : (
        <Skeleton lines={1} className="title" />
      )}
      {summary.tldr ? (
        <p className="sp-tldr">
          <Rich text={summary.tldr} />
          <Caret on={field === "tldr"} />
        </p>
      ) : (
        writing && <Skeleton lines={2} className="card" />
      )}

      {points.length > 0 && (
        <Section title="Key points">
          <ol className="sp-points">
            {points.map((point, i) => (
              <li key={i}>
                <span className="sp-num">{i + 1}</span>
                <span>
                  <Rich text={point} />
                  <Caret on={field === "points" && i === points.length - 1} />
                </span>
              </li>
            ))}
          </ol>
        </Section>
      )}
      {writing && !started("points") && (peek ? <Peek sentences={peek} /> : started("tldr") && <Skeleton lines={3} />)}

      {facts.length > 0 && (
        <Section title="Key figures">
          <div className="sp-facts">
            {facts.map((fact, i) => (
              <div key={i} className={cls("sp-fact", (fact.value?.length ?? 0) > 24 && "wide")}>
                <div className="sp-fact-value">{fact.value || "…"}</div>
                <div className="sp-fact-label">{fact.label}</div>
              </div>
            ))}
          </div>
        </Section>
      )}

      {actions.length > 0 && (
        <Section title="To do" count={actions.length}>
          <ul className="sp-todo">
            {actions.map((action, i) => (
              <li key={i} className={checked.has(i) ? "done" : ""} onClick={() => onCheck(i)}>
                <span className="sp-check">{checked.has(i) && <CheckIcon />}</span>
                <span>
                  <Rich text={action} />
                  <Caret on={field === "actions" && i === actions.length - 1} />
                </span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      {keywords.length > 0 && (
        <div className="sp-keywords">
          {keywords.map((keyword, i) => (
            <span key={i}>#{keyword.replace(/^#/, "")}</span>
          ))}
        </div>
      )}
      {writing && (
        <div className="sp-writing">
          <Sparkle className="live" />
          Writing…
        </div>
      )}

      {!writing && (answers.length > 0 || questions.length > 0) && (
        <Section title="Ask">
          {answers.length > 0 && <Answers answers={answers} />}
          {onAsk && (
            <div className="sp-suggestions">
              {questions
                .filter((q) => !answers.some((a) => a.question === q))
                .map((question, i) => (
                  <button key={i} onClick={() => onAsk(question)}>
                    {question}
                  </button>
                ))}
            </div>
          )}
        </Section>
      )}
    </article>
  );
}

/** Questions asked about the document, each answer streaming in under its question */
function Answers({ answers }: { answers: Answer[] }) {
  return (
    <div className="sp-answers">
      {answers.map((answer, i) => (
        <div key={i} className="sp-qa">
          <div className="sp-question">{answer.question}</div>
          <div className={cls("sp-answer", answer.error && "error")}>
            <Sparkle />
            {answer.error ? (
              <span>{answer.error}</span>
            ) : answer.text ? (
              <span>
                <Rich text={answer.text} />
                <Caret on={!answer.done} />
              </span>
            ) : (
              <span className="sp-thinking">
                <i />
                <i />
                <i />
              </span>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}
