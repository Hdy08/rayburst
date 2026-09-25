/** Native task paths and reactive file rechecks. */
import { shallowRef } from 'vue'
import type { Aria2Task } from '@shared/types'

export const recheckTrigger = shallowRef(0)

export function requestFileRecheck(): void {
  recheckTrigger.value++
}

/** Prefer the first selected file from the native task snapshot. */
export function resolveTaskFilePath(task: Aria2Task): string | null {
  const files = task.files
  if (!files || files.length === 0) return null
  return (files.find((file) => file.selected === 'true') ?? files[0])?.path ?? null
}
