const key = "corqty.returnTo"

// Storage can throw (disabled storage, private mode); returning is best effort.
export function rememberReturnTo(path: string) {
  try {
    sessionStorage.setItem(key, path)
  } catch {}
}

export function takeReturnTo(): string {
  let path: string | null = null
  try {
    path = sessionStorage.getItem(key)
    sessionStorage.removeItem(key)
  } catch {}
  return path?.startsWith("/") && !path.startsWith("//") ? path : "/"
}
