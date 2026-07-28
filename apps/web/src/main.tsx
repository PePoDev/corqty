import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { BrowserRouter } from "react-router-dom"

import { App } from "@/app"
import "@/styles/globals.css"

const systemTheme = window.matchMedia("(prefers-color-scheme: dark)")
function applySystemTheme() {
  document.documentElement.classList.toggle("dark", systemTheme.matches)
}
applySystemTheme()
systemTheme.addEventListener("change", applySystemTheme)

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </StrictMode>,
)
