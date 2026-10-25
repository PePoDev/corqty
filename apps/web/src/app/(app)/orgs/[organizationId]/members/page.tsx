import { MembersView } from "@/views/members-view"

export default async function MembersPage({
  params,
}: PageProps<"/orgs/[organizationId]/members">) {
  const { organizationId } = await params
  return <MembersView organizationId={organizationId} />
}
