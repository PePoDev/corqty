import type { Metadata } from "next"
import type { ReactNode } from "react"

import { Providers } from "@/app/providers"
import "./globals.css"

export const metadata: Metadata = { title: "Corqty" }

// Runs before hydration so the first paint already follows the system theme.
const themeScript = `(() => {
  const media = window.matchMedia("(prefers-color-scheme: dark)")
  const apply = () =>
    document.documentElement.classList.toggle("dark", media.matches)
  apply()
  media.addEventListener("change", apply)
})()`

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body>
        <Providers>{children}</Providers>
      </body>
    </html>
  )
}
