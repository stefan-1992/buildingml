import { useCallback, useEffect, useRef, useState } from 'react';
import { useChatStore, type ChatMessage } from '@state/chat-store.ts';
import { useAppStore } from '@state/app-store.ts';
import { useFloorplanStore } from '@state/floorplan-store.ts';
import { buildChatContext, extractBmlFromResponse, extractSearchReplaceBlocks, applySearchReplaceBlocks } from '@shared/chat/context-builder.ts';
import { useIsMobile } from '@shared/use-is-mobile.ts';

export function ChatPanel() {
  const messages = useChatStore((s) => s.messages);
  const isStreaming = useChatStore((s) => s.isStreaming);
  const streamBuffer = useChatStore((s) => s.streamBuffer);
  const addMessage = useChatStore((s) => s.addMessage);
  const updateLastAssistant = useChatStore((s) => s.updateLastAssistant);
  const setStreaming = useChatStore((s) => s.setStreaming);
  const setStreamBuffer = useChatStore((s) => s.setStreamBuffer);
  const clearMessages = useChatStore((s) => s.clearMessages);
  const toggleChat = useChatStore((s) => s.toggleOpen);
  const isMobile = useIsMobile();

  const source = useAppStore((s) => s.source);
  const setSource = useAppStore((s) => s.setSource);
  const provider = useAppStore((s) => s.provider);
  const viewMode = useAppStore((s) => s.viewMode);
  const diagnostics = useAppStore((s) => s.diagnostics);
  const setDiagnostics = useAppStore((s) => s.setDiagnostics);

  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const abortRef = useRef<AbortController | null>(null);

  // Auto-scroll to bottom when messages change
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, streamBuffer]);

  // Focus input on mount
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const applyBmlChanges = useCallback(async (bml: string) => {
    // Validate the BML first
    try {
      const diags = await provider.lint(bml);
      const errors = diags.filter(d => d.severity === 'error');
      if (errors.length > 0) {
        addMessage({
          role: 'system',
          content: `BML validation failed with ${errors.length} error(s): ${errors.slice(0, 3).map(e => e.message).join('; ')}. The changes were not applied.`,
        });
        return;
      }

      // Apply the new BML source
      setSource(bml);
      setDiagnostics(diags);
      addMessage({ role: 'system', content: 'Changes applied successfully.' });

      // Update the floorplan model
      try {
        const model = await provider.generateModel(bml);
        useFloorplanStore.getState().setOriginalFileSource(bml);
        useFloorplanStore.getState().loadFromModel(model, useFloorplanStore.getState().currentStoreyIndex);
      } catch {
        // Floorplan update may fail for some cases
      }

      // Rebuild 3D view
      try {
        const scene = await provider.generateScene(bml);
        const { useViewerStore } = await import('@state/viewer-store.ts');
        useViewerStore.getState().setScene(scene);
      } catch (sceneErr) {
        console.error('3D rebuild failed:', sceneErr);
      }
    } catch (e) {
      addMessage({
        role: 'system',
        content: `Failed to apply changes: ${e instanceof Error ? e.message : 'Unknown error'}`,
      });
    }
  }, [provider, setSource, setDiagnostics, addMessage]);

  const handleSend = useCallback(async () => {
    const trimmed = input.trim();
    if (!trimmed || isStreaming) return;

    addMessage({ role: 'user', content: trimmed });
    setInput('');
    setStreaming(true);
    setStreamBuffer('');

    // Build context for the LLM
    const floorplanState = useFloorplanStore.getState();
    let model = floorplanState.loadedModel;
    if (!model) {
      try {
        model = await provider.generateModel(source);
      } catch {
        // No model available
      }
    }

    const systemContext = buildChatContext({
      source,
      model,
      currentStoreyIndex: floorplanState.currentStoreyIndex,
      selectedElementId: floorplanState.selectedId,
      diagnostics,
      viewMode,
    });

    // Build messages array for OpenAI API
    const chatMessages: Array<{ role: 'system' | 'user' | 'assistant'; content: string }> = [
      { role: 'system', content: systemContext },
    ];

    // Include recent conversation history (last 10 messages)
    const recentMessages = messages.slice(-10);
    for (const msg of recentMessages) {
      if (msg.role === 'user' || msg.role === 'assistant') {
        chatMessages.push({ role: msg.role, content: msg.content });
      }
    }

    // Add the current user message
    chatMessages.push({ role: 'user', content: trimmed });

    // Add placeholder for streaming response
    addMessage({ role: 'assistant', content: '' });

    try {
      const abortController = new AbortController();
      abortRef.current = abortController;

      const result = await provider.chat(
        { messages: chatMessages },
        (chunk) => {
          setStreamBuffer(chunk);
          updateLastAssistant(chunk);
        },
      );

      // Final update with complete content
      const finalContent = result.content;
      updateLastAssistant(finalContent);

      // Check for SEARCH/REPLACE blocks first, then fall back to full BML replacement
      const srBlocks = extractSearchReplaceBlocks(finalContent);
      if (srBlocks.length > 0) {
        // Use fresh source from store (not stale closure)
        const currentSource = useAppStore.getState().source;
        const { result, errors } = applySearchReplaceBlocks(currentSource, srBlocks);
        if (errors.length > 0) {
          addMessage({
            role: 'system',
            content: `Some replacements failed:\n${errors.join('\n')}`,
          });
        }
        if (result !== currentSource) {
          await applyBmlChanges(result);
        }
      } else {
        const bml = extractBmlFromResponse(finalContent);
        if (bml) {
          await applyBmlChanges(bml);
        }
      }
    } catch (e) {
      const errMsg = e instanceof Error ? e.message : 'Unknown error';
      updateLastAssistant(`Error: ${errMsg}`);
    } finally {
      setStreaming(false);
      setStreamBuffer('');
      abortRef.current = null;
    }
  }, [input, isStreaming, source, messages, diagnostics, viewMode, provider, addMessage, updateLastAssistant, setStreaming, setStreamBuffer, applyBmlChanges]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
    // Prevent keyboard shortcuts from bubbling
    e.stopPropagation();
  }, [handleSend]);

  const handleStop = useCallback(() => {
    abortRef.current?.abort();
    setStreaming(false);
    setStreamBuffer('');
  }, [setStreaming, setStreamBuffer]);

  return (
    <div style={{
      display: 'flex', flexDirection: 'column',
      height: '100%', background: 'var(--surface)',
      borderLeft: isMobile ? 'none' : '1px solid var(--border)',
    }}>
      {/* Header */}
      <div style={{
        padding: isMobile ? '10px 12px' : '8px 12px',
        fontSize: isMobile ? 13 : 11, color: 'var(--text-dim)',
        borderBottom: '1px solid var(--border)',
        fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.5,
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
        minHeight: isMobile ? 44 : undefined,
      }}>
        <span>Chat</span>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <button
            onClick={clearMessages}
            title="Clear chat history"
            style={{
              background: 'transparent', border: 'none', color: 'var(--text-dim)',
              cursor: 'pointer', fontFamily: 'inherit', fontSize: isMobile ? 12 : 10,
              padding: isMobile ? '4px 8px' : '2px 6px',
            }}
          >
            Clear
          </button>
          {isMobile && (
            <button
              onClick={toggleChat}
              title="Close chat"
              style={{
                background: 'var(--border)', border: 'none', color: 'var(--text)',
                cursor: 'pointer', fontFamily: 'inherit', fontSize: 14,
                padding: '4px 10px', borderRadius: 4,
              }}
            >
              Close
            </button>
          )}
        </div>
      </div>

      {/* Messages */}
      <div style={{
        flex: 1, overflowY: 'auto', padding: '8px 12px',
        display: 'flex', flexDirection: 'column', gap: 8,
      }}>
        {messages.length === 0 && (
          <div style={{
            color: 'var(--text-dim)', fontSize: 11, lineHeight: 1.5,
            padding: '20px 0', textAlign: 'center',
          }}>
            Ask me to modify your building.<br />
            e.g. "Add a bathroom next to the kitchen"
          </div>
        )}
        {messages.map((msg) => (
          <MessageBubble key={msg.id} message={msg} />
        ))}
        <div ref={messagesEndRef} />
      </div>

      {/* Input */}
      <div style={{
        padding: isMobile ? '10px 12px' : '8px 12px',
        paddingBottom: isMobile ? 'max(10px, env(safe-area-inset-bottom))' : '8px',
        borderTop: '1px solid var(--border)',
        display: 'flex', gap: 6, alignItems: 'flex-end',
      }}>
        <textarea
          ref={inputRef}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Ask about your building..."
          rows={isMobile ? 2 : 2}
          disabled={isStreaming}
          style={{
            flex: 1, background: 'var(--bg)', color: 'var(--text)',
            border: '1px solid var(--border)', borderRadius: 4,
            padding: isMobile ? '10px 12px' : '6px 8px',
            fontFamily: 'inherit', fontSize: isMobile ? 14 : 11,
            lineHeight: 1.4, resize: 'none', outline: 'none',
            opacity: isStreaming ? 0.6 : 1,
          }}
        />
        {isStreaming ? (
          <button
            onClick={handleStop}
            style={{
              background: 'var(--error)', border: 'none', color: 'var(--bg)',
              padding: isMobile ? '10px 16px' : '6px 12px', borderRadius: 4, cursor: 'pointer',
              fontFamily: 'inherit', fontSize: isMobile ? 14 : 11, whiteSpace: 'nowrap',
              minHeight: isMobile ? 40 : undefined,
            }}
          >
            Stop
          </button>
        ) : (
          <button
            onClick={handleSend}
            disabled={!input.trim()}
            style={{
              background: input.trim() ? 'var(--accent)' : 'var(--border)',
              border: 'none', color: input.trim() ? 'var(--bg)' : 'var(--text-dim)',
              padding: isMobile ? '10px 16px' : '6px 12px', borderRadius: 4,
              cursor: input.trim() ? 'pointer' : 'default',
              fontFamily: 'inherit', fontSize: isMobile ? 14 : 11, whiteSpace: 'nowrap',
              minHeight: isMobile ? 40 : undefined,
            }}
          >
            Send
          </button>
        )}
      </div>
    </div>
  );
}

function MessageBubble({ message }: { message: ChatMessage }) {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';

  return (
    <div style={{
      alignSelf: isUser ? 'flex-end' : 'flex-start',
      maxWidth: '90%',
    }}>
      <div style={{
        fontSize: 10, color: 'var(--text-dim)',
        marginBottom: 2, textTransform: 'uppercase',
      }}>
        {isUser ? 'You' : isSystem ? 'System' : 'Assistant'}
      </div>
      <div style={{
        background: isUser
          ? 'rgba(137, 180, 250, 0.15)'
          : isSystem
          ? 'rgba(243, 139, 168, 0.1)'
          : 'var(--bg)',
        border: `1px solid ${isSystem ? 'rgba(243, 139, 168, 0.3)' : 'var(--border)'}`,
        borderRadius: 6,
        padding: '6px 10px',
        fontSize: 12,
        lineHeight: 1.5,
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        color: isSystem ? 'var(--error)' : 'var(--text)',
      }}>
        {formatMessageContent(message.content)}
      </div>
    </div>
  );
}

/** Render message content, highlighting code blocks and SEARCH/REPLACE blocks */
function formatMessageContent(content: string): React.ReactNode {
  if (!content) return <span style={{ color: 'var(--text-dim)' }}>...</span>;

  const parts = content.split(/(```(?:bml)?\n[\s\S]*?```|<<<<<<< SEARCH\n[\s\S]*?>>>>>>> REPLACE)/);
  return parts.map((part, i) => {
    const codeMatch = part.match(/```(?:bml)?\n([\s\S]*?)```/);
    if (codeMatch) {
      return (
        <pre key={i} style={{
          background: 'rgba(0,0,0,0.3)', borderRadius: 4,
          padding: '6px 8px', margin: '4px 0', fontSize: 10,
          overflow: 'auto', maxHeight: 200,
          border: '1px solid var(--border)',
        }}>
          <code>{codeMatch[1]}</code>
        </pre>
      );
    }
    const srMatch = part.match(/<<<<<<< SEARCH\n([\s\S]*?)\n=======\n([\s\S]*?)\n>>>>>>> REPLACE/);
    if (srMatch) {
      return (
        <div key={i} style={{ margin: '4px 0', borderRadius: 4, overflow: 'hidden', border: '1px solid var(--border)' }}>
          <pre style={{
            background: 'rgba(243, 139, 168, 0.1)', padding: '4px 8px',
            margin: 0, fontSize: 10, overflow: 'auto', maxHeight: 100,
            borderBottom: '1px solid var(--border)',
          }}>
            <code style={{ opacity: 0.7, textDecoration: 'line-through' }}>{srMatch[1]}</code>
          </pre>
          <pre style={{
            background: 'rgba(166, 227, 161, 0.1)', padding: '4px 8px',
            margin: 0, fontSize: 10, overflow: 'auto', maxHeight: 100,
          }}>
            <code>{srMatch[2]}</code>
          </pre>
        </div>
      );
    }
    return <span key={i}>{part}</span>;
  });
}
