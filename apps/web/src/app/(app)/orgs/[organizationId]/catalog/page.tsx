import { CatalogView } from "@/views/catalog-view"

export default async function CatalogPage({
  params,
}: PageProps<"/orgs/[organizationId]/catalog">) {
  const { organizationId } = await params
  return <CatalogView organizationId={organizationId} />
}
