/** Delete an explicitly selected path; task content deletion belongs to the native task service. */
import { invoke } from '@tauri-apps/api/core'
import type { FileDeletionMode } from '@shared/types'
export async function deletePath(path: string, mode: FileDeletionMode): Promise<boolean> {
  if (!path) return false
  return invoke<boolean>('delete_path', { path, mode })
}
