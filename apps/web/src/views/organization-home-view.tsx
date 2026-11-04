"use client"

import Link from "next/link"

export function OrganizationHomeView({
  organizationId,
}: {
  organizationId: string
}) {
  return (
    <section className="flex max-w-5xl flex-col gap-6 p-4 md:p-6">
      <div className="grid gap-2">
        <h1 className="text-2xl font-semibold tracking-tight">
          Your workspace
        </h1>
        <p className="text-sm text-muted-foreground">
          Build reusable workflows, publish a version, and follow every
          execution.
        </p>
      </div>
      <div className="grid gap-4 md:grid-cols-3">
        {[
          [
            "Workflows",
            "workflows",
            "Start with a blueprint or a blank canvas. Configure steps, validate the graph, and publish when it is ready.",
          ],
          [
            "Catalog",
            "catalog",
            "Explore reusable node definitions and blueprints. Editors can add definitions for installed runtime plugins.",
          ],
          [
            "Triggers",
            "triggers",
            "Run a published workflow on a schedule or from a signed webhook, pinned to the version you choose.",
          ],
        ].map(([title, route, description]) => (
          <Link
            key={route}
            href={`/orgs/${organizationId}/${route}`}
            className="grid content-start gap-3 rounded-lg border p-5 hover:bg-muted focus-visible:outline-2 focus-visible:outline-offset-2"
          >
            <h2 className="font-semibold">
              {title} <span aria-hidden="true">→</span>
            </h2>
            <p className="text-sm leading-relaxed text-muted-foreground">
              {description}
            </p>
          </Link>
        ))}
      </div>
      <p className="text-sm text-muted-foreground">
        Keep credentials in Secrets. Workspace admins manage members, runtime
        plugins, and service tokens from the navigation.
      </p>
    </section>
  )
}
