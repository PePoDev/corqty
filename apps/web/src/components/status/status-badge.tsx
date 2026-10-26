import { Badge } from "@/components/ui/badge"

// Status hues keep a fixed meaning, so this is the one place outside
// components/ui that may use raw palette colors.
const statuses = {
  saved: {
    label: "Saved",
    className: "border-emerald-600/40 text-emerald-700 dark:text-emerald-400",
  },
  saving: { label: "Saving…", className: "text-muted-foreground" },
  conflict: {
    label: "Conflict",
    className: "border-amber-600/40 text-amber-700 dark:text-amber-400",
  },
  error: {
    label: "Error",
    className: "border-red-600/40 text-red-700 dark:text-red-400",
  },
  simulated: {
    label: "Simulated",
    className: "border-blue-600/40 text-blue-700 dark:text-blue-400",
  },
  "read-only": { label: "Read-only", className: "text-muted-foreground" },
  // Run statuses (RunStatus).
  queued: { label: "Queued", className: "text-muted-foreground" },
  running: {
    label: "Running",
    className: "border-blue-600/40 text-blue-700 dark:text-blue-400",
  },
  succeeded: {
    label: "Succeeded",
    className: "border-emerald-600/40 text-emerald-700 dark:text-emerald-400",
  },
  failed: {
    label: "Failed",
    className: "border-red-600/40 text-red-700 dark:text-red-400",
  },
  canceled: { label: "Canceled", className: "text-muted-foreground" },
  // The other node-run states (NodeRunState).
  blocked: { label: "Blocked", className: "text-muted-foreground" },
  retry_wait: {
    label: "Retrying",
    className: "border-amber-600/40 text-amber-700 dark:text-amber-400",
  },
  skipped: { label: "Skipped", className: "text-muted-foreground" },
}

export type Status = keyof typeof statuses

export function StatusBadge({ status }: { status: Status }) {
  const { label, className } = statuses[status]
  return (
    <Badge variant="outline" className={className}>
      {label}
    </Badge>
  )
}
