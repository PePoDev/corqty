import { Navigate, Route, Routes, useParams } from "react-router-dom"

import { AuthProvider, useAuth } from "@/auth/auth-context"
import { ProtectedRoute } from "@/auth/protected-route"
import { AppShell } from "@/layout/app-shell"
import { AcceptInvitationPage } from "@/pages/accept-invitation-page"
import { AuditPage } from "@/pages/audit-page"
import { LoginPage } from "@/pages/login-page"
import { MembersPage } from "@/pages/members-page"
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

function useCurrentMembership() {
  const { organizationId } = useParams()
  const { me } = useAuth()
  return me?.organizations.find(
    (organization) => organization.organizationId === organizationId,
  )
}

function MembersRoute() {
  const membership = useCurrentMembership()
  const { me } = useAuth()
  return membership && me ? (
    <MembersPage
      organizationId={membership.organizationId}
      currentRole={membership.role}
      currentUserId={me.user.id}
    />
  ) : (
    <Navigate replace to="/" />
  )
}

function AuditRoute() {
  const membership = useCurrentMembership()
  return membership ? (
    <AuditPage organizationId={membership.organizationId} />
  ) : (
    <Navigate replace to="/" />
  )
}

export function App() {
  return (
    <AuthProvider>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/register" element={<RegisterPage />} />
        <Route element={<ProtectedRoute />}>
          <Route
            path="/invitations/accept"
            element={<AcceptInvitationPage />}
          />
          <Route path="/orgs/:organizationId" element={<AppShell />}>
            <Route index element={<OrganizationHomePage />} />
            <Route path="members" element={<MembersRoute />} />
            <Route path="audit" element={<AuditRoute />} />
          </Route>
          <Route path="/" element={<NavigateToFirstOrganization />} />
        </Route>
      </Routes>
    </AuthProvider>
  )
}
