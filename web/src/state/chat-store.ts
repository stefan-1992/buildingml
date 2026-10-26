import { create } from 'zustand';

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  timestamp: number;
  /** If the assistant response contained BML changes that were applied */
  appliedBml?: string;
  /** If there was an error applying the BML changes */
  error?: string;
}

interface ChatState {
  messages: ChatMessage[];
  isOpen: boolean;
  isStreaming: boolean;
  /** Accumulated partial response while streaming */
  streamBuffer: string;

  toggleOpen: () => void;
  setOpen: (open: boolean) => void;
  addMessage: (msg: Omit<ChatMessage, 'id' | 'timestamp'>) => void;
  updateLastAssistant: (content: string) => void;
  setStreaming: (streaming: boolean) => void;
  setStreamBuffer: (buf: string) => void;
  clearMessages: () => void;
}

let nextId = 0;

export const useChatStore = create<ChatState>((set) => ({
  messages: [],
  isOpen: false,
  isStreaming: false,
  streamBuffer: '',

  toggleOpen: () => set((s) => ({ isOpen: !s.isOpen })),
  setOpen: (open) => set({ isOpen: open }),

  addMessage: (msg) => set((s) => ({
    messages: [...s.messages, {
      ...msg,
      id: `msg-${++nextId}`,
      timestamp: Date.now(),
    }],
  })),

  updateLastAssistant: (content) => set((s) => {
    const msgs = [...s.messages];
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].role === 'assistant') {
        msgs[i] = { ...msgs[i], content };
        break;
      }
    }
    return { messages: msgs };
  }),

  setStreaming: (streaming) => set({ isStreaming: streaming }),
  setStreamBuffer: (buf) => set({ streamBuffer: buf }),

  clearMessages: () => set({ messages: [], streamBuffer: '' }),
}));
