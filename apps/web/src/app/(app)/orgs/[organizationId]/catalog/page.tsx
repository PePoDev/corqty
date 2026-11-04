import { CatalogView } from "@/views/catalog-view"

export default async function CatalogPage({
  params,
  searchParams,
}: PageProps<"/orgs/[organizationId]/catalog">) {
  const { organizationId } = await params
  const query = await searchParams
  const initialExecutor =
    typeof query.executor === "string" &&
    typeof query.executorVersion === "string"
      ? { key: query.executor, version: query.executorVersion }
      : undefined
  return (
    <CatalogView
      organizationId={organizationId}
      initialExecutor={initialExecutor}
    />
  )
}
