import { TriggersView } from "@/views/triggers-view"

export default async function TriggersPage({
  params,
}: PageProps<"/orgs/[organizationId]/triggers">) {
  const { organizationId } = await params
  return <TriggersView key={organizationId} organizationId={organizationId} />
}
