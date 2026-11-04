import { SecretsView } from "@/views/secrets-view"

export default async function SecretsPage({
  params,
}: PageProps<"/orgs/[organizationId]/secrets">) {
  const { organizationId } = await params
  return <SecretsView key={organizationId} organizationId={organizationId} />
}
