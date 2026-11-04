import { ServiceTokensView } from "@/views/service-tokens-view"

export default async function ServiceTokensPage({
  params,
}: PageProps<"/orgs/[organizationId]/service-tokens">) {
  const { organizationId } = await params
  return (
    <ServiceTokensView key={organizationId} organizationId={organizationId} />
  )
}
