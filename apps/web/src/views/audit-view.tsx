"use client"

import { useAuditEvents } from "@/api/queries"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

export function AuditView({ organizationId }: { organizationId: string }) {
  const auditEvents = useAuditEvents(organizationId)

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Audit log</h1>
        <p className="text-sm text-muted-foreground">
          Append-only administrative history.
        </p>
      </div>
      {auditEvents.isPending && (
        <p className="text-sm text-muted-foreground">Loading audit events…</p>
      )}
      {auditEvents.isError && (
        <p role="alert" className="text-sm text-destructive">
          The audit log could not be loaded.
        </p>
      )}
      <div className="overflow-x-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Time</TableHead>
              <TableHead>Action</TableHead>
              <TableHead>Resource</TableHead>
              <TableHead>Actor</TableHead>
              <TableHead>Metadata</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {auditEvents.data?.items.map((item) => (
              <TableRow key={item.id}>
                <TableCell>
                  {new Date(item.occurredAt).toLocaleString()}
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {item.action}
                </TableCell>
                <TableCell>
                  {item.resourceType}:{item.resourceId}
                </TableCell>
                <TableCell>{item.actorUserId ?? "system"}</TableCell>
                <TableCell className="font-mono text-xs">
                  {JSON.stringify(item.metadata)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}
