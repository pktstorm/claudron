import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";

/// Render transcript text as markdown.
///
/// `react-markdown` does not pass raw HTML through unless `rehype-raw` is added,
/// which is deliberately NOT used here: transcript content is untrusted -- it
/// carries whatever the model wrote and whatever tool output was captured, and
/// a session's own text should never be able to inject markup into Claudron.
///
/// Styling is applied per element rather than through a prose plugin, so the
/// palette stays matched to the surrounding dark UI.
export function Markdown({ text }: { text: string }) {
  return (
    <div className="text-sm leading-relaxed text-neutral-200">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        components={{
          p: ({ children }) => <p className="mb-2 break-words last:mb-0">{children}</p>,
          h1: ({ children }) => <h1 className="mb-2 mt-3 text-base font-semibold text-neutral-100">{children}</h1>,
          h2: ({ children }) => <h2 className="mb-2 mt-3 text-sm font-semibold text-neutral-100">{children}</h2>,
          h3: ({ children }) => <h3 className="mb-1 mt-2 text-sm font-semibold text-neutral-100">{children}</h3>,
          ul: ({ children }) => <ul className="mb-2 list-disc space-y-0.5 pl-5 last:mb-0">{children}</ul>,
          ol: ({ children }) => <ol className="mb-2 list-decimal space-y-0.5 pl-5 last:mb-0">{children}</ol>,
          li: ({ children }) => <li className="break-words">{children}</li>,
          strong: ({ children }) => <strong className="font-semibold text-neutral-100">{children}</strong>,
          em: ({ children }) => <em className="italic">{children}</em>,
          a: ({ children, href }) => (
            <a
              href={href}
              target="_blank"
              rel="noreferrer noopener"
              className="text-sky-400 underline underline-offset-2 hover:text-sky-300"
            >
              {children}
            </a>
          ),
          blockquote: ({ children }) => (
            <blockquote className="mb-2 border-l-2 border-neutral-700 pl-3 text-neutral-400 last:mb-0">
              {children}
            </blockquote>
          ),
          hr: () => <hr className="my-3 border-neutral-800" />,
          // A fenced block arrives as <pre><code>; an inline span as a bare
          // <code>. Only the latter gets a background pill, or the pill would
          // double up inside the block's own frame.
          code: ({ className, children, ...props }) => {
            const fenced = /language-/.test(className ?? "");
            if (fenced) {
              return (
                <code className={`${className ?? ""} block`} {...props}>
                  {children}
                </code>
              );
            }
            return (
              <code className="rounded bg-neutral-800 px-1 py-0.5 font-mono text-[0.85em] text-neutral-200">
                {children}
              </code>
            );
          },
          pre: ({ children }) => (
            <pre className="mb-2 overflow-x-auto rounded border border-neutral-800 bg-neutral-900/80 p-2.5 font-mono text-xs last:mb-0">
              {children}
            </pre>
          ),
          // Tables can be far wider than the pane; scroll them rather than
          // letting one force the whole conversation to scroll sideways.
          table: ({ children }) => (
            <div className="mb-2 overflow-x-auto last:mb-0">
              <table className="w-full border-collapse text-xs">{children}</table>
            </div>
          ),
          th: ({ children }) => (
            <th className="border border-neutral-800 bg-neutral-800/50 px-2 py-1 text-left font-semibold">
              {children}
            </th>
          ),
          td: ({ children }) => <td className="border border-neutral-800 px-2 py-1 align-top">{children}</td>,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
