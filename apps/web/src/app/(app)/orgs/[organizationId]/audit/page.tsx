import { AuditView } from "@/views/audit-view"

export default async function AuditPage({
  params,
}: PageProps<"/orgs/[organizationId]/audit">) {
  const { organizationId } = await params
  return <AuditView organizationId={organizationId} />
}
