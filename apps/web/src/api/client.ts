function csrfToken(): string | undefined {
  return document.cookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("corqty_csrf="))
    ?.split("=")
    .slice(1)
    .join("=")
}

export async function api<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers)
  if (init.body) headers.set("content-type", "application/json")
  const csrf = csrfToken()
  if (csrf && init.method && init.method !== "GET") {
    headers.set("x-csrf-token", csrf)
  }
  const response = await fetch(`/api/v1${path}`, {
    ...init,
    headers,
    credentials: "include",
  })
  if (!response.ok) {
    const problem = (await response
      .json()
      .catch(() => ({ code: "unknown" }))) as { code?: string }
    throw new ApiProblem(response.status, problem.code ?? "unknown")
  }
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

export class ApiProblem extends Error {
  constructor(
    public readonly status: number,
    public readonly code: string,
  ) {
    super(`api:${status}:${code}`)
  }
}
