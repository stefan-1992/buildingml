import { useRef, useCallback, useState } from 'react';
import { useAppStore } from '@state/app-store.ts';
import { BmlCodeMirror } from '@shared/codemirror/BmlCodeMirror.tsx';
import type { Diagnostic } from '@shared/types.ts';

export function CodeEditor() {
  const source = useAppStore((s) => s.source);
  const setSource = useAppStore((s) => s.setSource);
  const lintTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const provider = useAppStore((s) => s.provider);
  const setDiagnostics = useAppStore((s) => s.setDiagnostics);
  const [localDiags, setLocalDiags] = useState<Diagnostic[]>([]);

  const handleChange = useCallback((value: string) => {
    setSource(value);

    // Debounced lint
    if (lintTimeoutRef.current) clearTimeout(lintTimeoutRef.current);
    lintTimeoutRef.current = setTimeout(async () => {
      try {
        const diags = await provider.lint(value);
        setDiagnostics(diags);
        setLocalDiags(diags.filter(d => d.severity === 'error'));
      } catch {
        // Lint errors are non-fatal
      }
    }, 500);
  }, [setSource, provider, setDiagnostics]);

  return (
    <div style={{
      display: 'flex', flexDirection: 'column',
      background: 'var(--surface)', height: '100%',
    }}>
      <div style={{
        padding: '6px 12px', fontSize: 11, color: 'var(--text-dim)',
        borderBottom: '1px solid var(--border)',
      }}>
        editor.bml
      </div>
      <BmlCodeMirror
        value={source}
        onChange={handleChange}
        diagnostics={localDiags}
        stopKeyPropagation={false}
      />
    </div>
  );
}
