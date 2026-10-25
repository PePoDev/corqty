import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render } from "@testing-library/react"
import type { ReactElement } from "react"
import { vi } from "vitest"

import { AuthProvider } from "@/auth/auth-context"

// setup.ts mocks next/navigation: useRouter returns this router and
// usePathname reads window.location.
export const router = { push: vi.fn(), replace: vi.fn() }

// The providers wrap every rerender, so the query cache outlives a page.
export function renderWithProviders(ui: ReactElement, { route = "/" } = {}) {
  window.history.replaceState(null, "", route)
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return render(ui, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={queryClient}>
        <AuthProvider>{children}</AuthProvider>
      </QueryClientProvider>
    ),
  })
}
