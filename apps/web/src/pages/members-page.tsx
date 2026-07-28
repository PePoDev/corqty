import { useCallback, useEffect, useState } from "react"
import type { FormEvent } from "react"

import { ApiProblem, api } from "@/api/client"
import type { Role } from "@/api/types"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

interface Member {
  userId: string
  email: string
  role: Role
}

interface Invitation {
  token: string
  inviteUrl?: string
  expiresAt: string
}

const roles: Role[] = ["admin", "editor", "runner", "viewer"]

export function MembersPage({
  organizationId,
  currentRole,
  currentUserId,
}: {
  organizationId: string
  currentRole: Role
  currentUserId: string
}) {
  const [members, setMembers] = useState<Member[]>([])
  const [showInvite, setShowInvite] = useState(false)
  const [email, setEmail] = useState("")
  const [role, setRole] = useState<Role>("viewer")
  const [invitation, setInvitation] = useState<Invitation | null>(null)
  const [error, setError] = useState("")
  const admin = currentRole === "admin"

  const load = useCallback(async () => {
    setMembers(await api<Member[]>(`/orgs/${organizationId}/members`))
  }, [organizationId])

  useEffect(() => {
    void load()
  }, [load])

  async function invite(event: FormEvent) {
    event.preventDefault()
    setError("")
    try {
      const created = await api<Invitation>(
        `/orgs/${organizationId}/invitations`,
        { method: "POST", body: JSON.stringify({ email, role }) },
      )
      setInvitation(created)
      setEmail("")
    } catch {
      setError("The invitation could not be created.")
    }
  }

  async function changeRole(userId: string, nextRole: Role) {
    setError("")
    try {
      await api(`/orgs/${organizationId}/members/${userId}`, {
        method: "PATCH",
        body: JSON.stringify({ role: nextRole }),
      })
      await load()
    } catch (problem) {
      setError(
        problem instanceof ApiProblem && problem.code === "final_admin_required"
          ? "Every organization must keep one admin."
          : "The member role could not be changed.",
      )
    }
  }

  async function remove(userId: string) {
    setError("")
    try {
      await api(`/orgs/${organizationId}/members/${userId}`, {
        method: "DELETE",
      })
      await load()
    } catch (problem) {
      setError(
        problem instanceof ApiProblem && problem.code === "final_admin_required"
          ? "Every organization must keep one admin."
          : "The member could not be removed.",
      )
    }
  }

  async function copyInvitation() {
    if (!invitation) return
    try {
      await navigator.clipboard.writeText(
        invitation.inviteUrl ?? invitation.token,
      )
    } catch {
      setError("The invitation link could not be copied.")
    }
  }

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <header className="flex flex-col items-start justify-between gap-3 sm:flex-row sm:items-center">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Members</h1>
          <p className="text-sm text-muted-foreground">
            Fixed organization roles and invitations.
          </p>
        </div>
        {admin && (
          <Button onClick={() => setShowInvite(true)}>Invite member</Button>
        )}
      </header>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {admin && (
        <Dialog
          open={showInvite}
          onOpenChange={(open) => {
            setShowInvite(open)
            if (!open) setInvitation(null)
          }}
        >
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Create invitation</DialogTitle>
              <DialogDescription>
                Choose an email and fixed organization role.
              </DialogDescription>
            </DialogHeader>
            <form className="grid gap-4" onSubmit={invite}>
              <Field>
                <FieldLabel htmlFor="invite-email">Email</FieldLabel>
                <Input
                  id="invite-email"
                  type="email"
                  required
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="invite-role">Role</FieldLabel>
                <select
                  id="invite-role"
                  className="h-9 rounded-md border bg-background px-3 text-sm"
                  value={role}
                  onChange={(event) => setRole(event.target.value as Role)}
                >
                  {roles.map((value) => (
                    <option key={value} value={value}>
                      {value}
                    </option>
                  ))}
                </select>
              </Field>
              <Button className="sm:justify-self-end" type="submit">
                Create invitation
              </Button>
            </form>
            {invitation && (
              <div
                aria-live="polite"
                className="rounded-md border bg-muted/40 p-3"
              >
                <p className="text-xs text-muted-foreground">
                  Shown once; expires {invitation.expiresAt}
                </p>
                <div className="mt-2 flex items-center gap-2">
                  <code className="min-w-0 flex-1 break-all text-xs">
                    {invitation.inviteUrl ?? invitation.token}
                  </code>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => void copyInvitation()}
                  >
                    Copy link
                  </Button>
                </div>
              </div>
            )}
          </DialogContent>
        </Dialog>
      )}
      <Card className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Email</TableHead>
              <TableHead>Role</TableHead>
              <TableHead>
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {members.map((member) => (
              <TableRow key={member.userId}>
                <TableCell>{member.email}</TableCell>
                <TableCell>
                  {admin ? (
                    <select
                      aria-label={`Role for ${member.email}`}
                      className="h-8 rounded-md border bg-background px-2 text-sm"
                      value={member.role}
                      onChange={(event) =>
                        void changeRole(
                          member.userId,
                          event.target.value as Role,
                        )
                      }
                    >
                      {roles.map((value) => (
                        <option key={value} value={value}>
                          {value}
                        </option>
                      ))}
                    </select>
                  ) : (
                    member.role
                  )}
                </TableCell>
                <TableCell className="text-right">
                  {admin && member.userId !== currentUserId && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void remove(member.userId)}
                    >
                      Remove
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </Card>
    </section>
  )
}
