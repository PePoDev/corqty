import { TriangleAlertIcon } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"

// Shown where shell is enabled and where a shell node is configured.
export function ShellWarning() {
  return (
    <Alert>
      <TriangleAlertIcon />
      <AlertTitle>Trusted users only</AlertTitle>
      <AlertDescription>
        Shell commands run inside the worker container and can use anything it
        can reach. The shell executor is not a security boundary, so only
        trusted people should be able to edit workflows.
      </AlertDescription>
    </Alert>
  )
}
