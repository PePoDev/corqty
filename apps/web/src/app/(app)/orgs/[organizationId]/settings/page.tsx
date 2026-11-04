import { SettingsView } from "@/views/settings-view"

export default async function SettingsPage({
  params,
}: PageProps<"/orgs/[organizationId]/settings">) {
  const { organizationId } = await params
  return <SettingsView organizationId={organizationId} />
}
