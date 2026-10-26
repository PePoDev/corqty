import { RunView } from "@/runs/run-view"

export default async function RunPage({
  params,
}: PageProps<"/orgs/[organizationId]/runs/[runId]">) {
  const { organizationId, runId } = await params
  // A new run (a rerun) starts with its own graph and selection.
  return <RunView key={runId} organizationId={organizationId} runId={runId} />
}
