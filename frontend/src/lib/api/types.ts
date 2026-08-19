export interface ApiErrorBody { code: string; message: string; requestId?: string; fields?: Record<string, string> }

export interface ApiPage<T> { data: T[]; pagination: { hasMore: boolean; nextCursor?: string } }

export interface SessionUser { id: string; email: string; name: string; role: 'owner' | 'admin' | 'developer' | 'analyst' }

export interface WorkspaceContext { id: string; name: string; slug: string; plan: string; usage: { sent: number; limit: number } }

export interface ApiContext { user: SessionUser; workspace: WorkspaceContext }

