import { WorkflowsView } from "@/views/workflows-view"

export default async function WorkflowsPage({
  params,
}: PageProps<"/orgs/[organizationId]/workflows">) {
  const { organizationId } = await params
  return <WorkflowsView organizationId={organizationId} />
}
