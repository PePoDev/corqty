import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react"
import type { ReactNode } from "react"

import { ApiProblem, api } from "@/api/client"
import type { Me } from "@/api/types"

interface AuthValue {
  me: Me | null
  loading: boolean
  // The last session check failed for a reason other than 401.
  error: boolean
  refresh: () => Promise<void>
}

const AuthContext = createContext<AuthValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [me, setMe] = useState<Me | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(false)
  const refresh = useCallback(async () => {
    try {
      setMe(await api<Me>("/auth/me"))
      setError(false)
    } catch (problem) {
      const signedOut = problem instanceof ApiProblem && problem.status === 401
      if (signedOut) setMe(null)
      setError(!signedOut)
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    void refresh()
  }, [refresh])
  const value = useMemo(
    () => ({ me, loading, error, refresh }),
    [me, loading, error, refresh],
  )
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthValue {
  const value = useContext(AuthContext)
  if (!value) throw new Error("useAuth must be used inside AuthProvider")
  return value
}
