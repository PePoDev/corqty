import { Studio } from "@/studio/studio"

export default async function WorkflowPage({
  params,
}: PageProps<"/orgs/[organizationId]/workflows/[workflowId]">) {
  const { organizationId, workflowId } = await params
  return <Studio organizationId={organizationId} workflowId={workflowId} />
}
