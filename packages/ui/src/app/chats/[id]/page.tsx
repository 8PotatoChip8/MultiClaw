'use client';
import { useParams } from 'next/navigation';
import Chat from '../../../components/Chat';
import Link from 'next/link';

export default function ChatDetailPage() {
    const params = useParams();
    const id = params?.id as string;

    return (
        <div className="animate-in" style={{ height: 'calc(100vh - 96px)' }}>
            <div style={{ marginBottom: '16px' }}>
                <Link href="/chats" style={{ fontSize: '13px', color: 'var(--text-muted)' }}>← All Chats</Link>
            </div>
            <div style={{ height: 'calc(100% - 40px)' }}>
                <Chat threadId={id} initialMessages={[]} />
            </div>
        </div>
    );
}
