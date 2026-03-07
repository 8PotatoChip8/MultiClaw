'use client';
import ReactMarkdown from 'react-markdown';

export default function MarkdownText({ children }: { children: string }) {
    return (
        <div className="markdown-text">
            <ReactMarkdown>{children}</ReactMarkdown>
        </div>
    );
}
