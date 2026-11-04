import { PluginsView } from "@/views/plugins-view"

export default async function PluginsPage({
  params,
}: PageProps<"/orgs/[organizationId]/plugins">) {
  const { organizationId } = await params
  return <PluginsView key={organizationId} organizationId={organizationId} />
}
