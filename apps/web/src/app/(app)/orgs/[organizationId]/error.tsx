"use client"

import { Button } from "@/components/ui/button"

// A page that crashes leaves the app shell around it in place.
export default function OrganizationError({ reset }: { reset: () => void }) {
  return (
    <div className="grid justify-items-start gap-3 p-4 text-sm md:p-6">
      <p role="alert">Something went wrong on this page.</p>
      <Button variant="outline" size="sm" onClick={() => reset()}>
        Retry
      </Button>
    </div>
  )
}
