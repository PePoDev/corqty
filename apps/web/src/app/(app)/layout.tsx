import type { ReactNode } from "react"

import { AuthGate } from "@/auth/auth-gate"

export default function SignedInLayout({ children }: { children: ReactNode }) {
  return <AuthGate>{children}</AuthGate>
}
