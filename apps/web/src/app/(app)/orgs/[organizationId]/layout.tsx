import { AppShell } from "@/layout/app-shell"

export default async function OrganizationLayout({
  children,
  params,
}: LayoutProps<"/orgs/[organizationId]">) {
  const { organizationId } = await params
  return <AppShell organizationId={organizationId}>{children}</AppShell>
}
