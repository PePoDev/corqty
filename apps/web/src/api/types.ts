export type Role = "admin" | "editor" | "runner" | "viewer"

export interface OrganizationMembership {
  organizationId: string
  name: string
  slug: string
  role: Role
}

export interface Me {
  user: { id: string; email: string }
  organizations: OrganizationMembership[]
}
