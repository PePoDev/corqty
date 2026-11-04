import { OrganizationHomeView } from "@/views/organization-home-view"

export default async function OrganizationPage({
  params,
}: PageProps<"/orgs/[organizationId]">) {
  const { organizationId } = await params
  return <OrganizationHomeView organizationId={organizationId} />
}
