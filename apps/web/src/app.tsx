import { Navigate, Route, Routes } from "react-router-dom"

import { AuthProvider, useAuth } from "@/auth/auth-context"
import { ProtectedRoute } from "@/auth/protected-route"
import { AppShell } from "@/layout/app-shell"
import { LoginPage } from "@/pages/login-page"
import { OrganizationHomePage } from "@/pages/organization-home-page"
import { RegisterPage } from "@/pages/register-page"

function NavigateToFirstOrganization() {
  const { me } = useAuth()
  const first = me?.organizations[0]
  if (!first) {
    return (
      <p className="p-6 text-sm text-muted-foreground">
        No organization memberships.
      </p>
    )
  }
  return <Navigate replace to={`/orgs/${first.organizationId}`} />
}

export function App() {
  return (
    <AuthProvider>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/register" element={<RegisterPage />} />
        <Route element={<ProtectedRoute />}>
          <Route path="/orgs/:organizationId" element={<AppShell />}>
            <Route index element={<OrganizationHomePage />} />
          </Route>
          <Route path="/" element={<NavigateToFirstOrganization />} />
        </Route>
      </Routes>
    </AuthProvider>
  )
}
