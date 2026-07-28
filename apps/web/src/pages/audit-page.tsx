import { useEffect, useState } from "react"

import { api } from "@/api/client"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

interface AuditEvent {
  id: number
  actorUserId: string | null
  action: string
  resourceType: string
  resourceId: string
  metadata: Record<string, unknown>
  occurredAt: string
}

export function AuditPage({ organizationId }: { organizationId: string }) {
  const [items, setItems] = useState<AuditEvent[]>([])

  useEffect(() => {
    void api<{ items: AuditEvent[] }>(
      `/orgs/${organizationId}/audit-events?limit=50`,
    ).then((response) => setItems(response.items))
  }, [organizationId])

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Audit log</h1>
        <p className="text-sm text-muted-foreground">
          Append-only administrative history.
        </p>
      </div>
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
            {items.map((item) => (
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
