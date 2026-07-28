import { useParams } from "react-router-dom"

export function OrganizationHomePage() {
  const { organizationId } = useParams()
  return (
    <section className="flex flex-col gap-2 p-4 md:p-6">
      <p className="text-xs font-medium tracking-wider text-muted-foreground uppercase">
        Foundation
      </p>
      <h1 className="text-2xl font-semibold tracking-tight">
        Organization ready
      </h1>
      <p className="text-sm text-muted-foreground">
        Tenant context: {organizationId}
      </p>
    </section>
  )
}
