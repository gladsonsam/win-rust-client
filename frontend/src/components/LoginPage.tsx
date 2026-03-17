import { useState, useRef, useEffect } from 'react'
import { Lock, Loader2 } from 'lucide-react'
import { api } from '../lib/api'
import { cn } from '../lib/utils'

interface Props {
  onSuccess: () => void
}

export function LoginPage({ onSuccess }: Props) {
  const [password, setPassword] = useState('')
  const [error,    setError]    = useState('')
  const [loading,  setLoading]  = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => { inputRef.current?.focus() }, [])

  const submit = async () => {
    if (!password || loading) return
    setLoading(true)
    setError('')
    try {
      await api.login(password)
      onSuccess()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Login failed')
      setPassword('')
      inputRef.current?.focus()
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-bg flex items-center justify-center p-4">
      <div className="w-full max-w-sm">

        {/* Header */}
        <div className="flex flex-col items-center gap-3 mb-8">
          <div className="w-12 h-12 rounded-xl bg-accent/15 flex items-center justify-center">
            <Lock size={22} className="text-accent" />
          </div>
          <div className="text-center">
            <h1 className="text-xl font-semibold text-primary">Monitor Dashboard</h1>
            <p className="text-sm text-muted mt-1">Enter your password to continue</p>
          </div>
        </div>

        {/* Card */}
        <div className="bg-surface border border-border rounded-xl p-6 shadow-lg">
          <div className="flex flex-col gap-4">

            {/* Password field */}
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-muted uppercase tracking-wide">
                Password
              </label>
              <input
                ref={inputRef}
                type="password"
                value={password}
                onChange={e => { setPassword(e.target.value); setError('') }}
                onKeyDown={e => e.key === 'Enter' && submit()}
                placeholder="••••••••"
                className={cn(
                  'w-full px-3 py-2.5 rounded-lg text-sm bg-bg',
                  'border transition-colors outline-none',
                  'placeholder:text-muted/40',
                  error
                    ? 'border-danger focus:border-danger'
                    : 'border-border focus:border-accent',
                )}
              />
            </div>

            {/* Error */}
            {error && (
              <p className="text-xs text-danger flex items-center gap-1.5">
                <span className="w-1.5 h-1.5 rounded-full bg-danger flex-shrink-0" />
                {error}
              </p>
            )}

            {/* Submit */}
            <button
              onClick={submit}
              disabled={!password || loading}
              className={cn(
                'w-full flex items-center justify-center gap-2',
                'py-2.5 rounded-lg text-sm font-medium transition-colors',
                'bg-accent text-white',
                'hover:bg-accent/90 active:scale-[0.98]',
                'disabled:opacity-40 disabled:cursor-not-allowed',
              )}
            >
              {loading
                ? <><Loader2 size={14} className="animate-spin" /> Signing in…</>
                : 'Sign in'}
            </button>

          </div>
        </div>

      </div>
    </div>
  )
}
