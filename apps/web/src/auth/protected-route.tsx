import { Navigate, Outlet, useLocation } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"

export function ProtectedRoute() {
  const { me, loading } = useAuth()
  const location = useLocation()
  if (loading) {
    return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
  }
  if (!me) return <Navigate to="/login" replace state={{ from: location }} />
  return <Outlet />
}
