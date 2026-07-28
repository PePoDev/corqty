import { useEffect, useState } from "react"
import type { FormEvent } from "react"
import { Link, useLocation, useNavigate } from "react-router-dom"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"

export function LoginPage() {
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [error, setError] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const { me, refresh } = useAuth()
  const location = useLocation()
  const navigate = useNavigate()

  useEffect(() => {
    if (!me) return
    const from = (
      location.state as {
        from?: { pathname: string; search: string; hash: string }
      } | null
    )?.from
    navigate(from ? `${from.pathname}${from.search}${from.hash}` : "/", {
      replace: true,
    })
  }, [location.state, me, navigate])

  async function submit(event: FormEvent) {
    event.preventDefault()
    setSubmitting(true)
    setError("")
    try {
      await api("/auth/login", {
        method: "POST",
        body: JSON.stringify({ email, password }),
      })
      await refresh()
    } catch {
      setError("Email or password is incorrect.")
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <main className="grid min-h-screen place-items-center bg-muted/30 p-6">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>
            <h1>Sign in to Corqty</h1>
          </CardTitle>
          <CardDescription>Continue to your workflow workspace.</CardDescription>
        </CardHeader>
        <CardContent>
          <form className="flex flex-col gap-5" onSubmit={submit}>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="email">Email</FieldLabel>
                <Input
                  id="email"
                  type="email"
                  autoComplete="email"
                  required
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="password">Password</FieldLabel>
                <Input
                  id="password"
                  type="password"
                  autoComplete="current-password"
                  required
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                />
              </Field>
            </FieldGroup>
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
            <Button type="submit" disabled={submitting}>
              {submitting ? "Signing in…" : "Sign in"}
            </Button>
            <p className="text-center text-sm text-muted-foreground">
              New to Corqty?{" "}
              <Link
                className="text-foreground underline"
                state={location.state}
                to="/register"
              >
                Create an account
              </Link>
            </p>
          </form>
        </CardContent>
      </Card>
    </main>
  )
}
