/** Shared native availability state; cards never inspect individual paths. */
import { computed, onBeforeUnmount, shallowRef, watch, type ComputedRef } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { recheckTrigger } from '@/composables/useTaskPaths'
import { logger } from '@shared/logger'
import type { Aria2Task } from '@shared/types'

export type TaskFileState = 'available' | 'missing' | 'inaccessible' | 'unknown'
const states = shallowRef<Record<string, TaskFileState>>({})
const visible = new Map<string, number>()
let scheduled = false
let revision = 0
export function updateTaskFileStates(value: Record<string, TaskFileState>): void {
  revision++
  states.value = value
}

function inspect(): void {
  if (scheduled) return
  scheduled = true
  queueMicrotask(async () => {
    scheduled = false
    const gids = [...visible.keys()]
    const startedAt = revision
    try {
      const result = await invoke<Record<string, TaskFileState>>('task_file_states', { gids })
      if (revision === startedAt) updateTaskFileStates(result)
    } catch (error) {
      logger.debug('TaskFiles.inspect', error)
    }
  })
}

export function useTaskFileMissing(task: ComputedRef<Aria2Task>) {
  const release = (gid: string) => {
    const count = (visible.get(gid) ?? 1) - 1
    if (count > 0) visible.set(gid, count)
    else visible.delete(gid)
  }
  watch(
    () => task.value.gid,
    (gid, previous) => {
      if (previous) release(previous)
      visible.set(gid, (visible.get(gid) ?? 0) + 1)
      inspect()
    },
    { immediate: true },
  )
  watch([() => task.value.status, () => task.value.seeder, recheckTrigger], inspect)
  onBeforeUnmount(() => {
    release(task.value.gid)
    inspect()
  })
  const fileState = computed(() => states.value[task.value.gid] ?? 'unknown')
  const fileMissing = computed(() => fileState.value === 'missing' || fileState.value === 'inaccessible')
  return { fileMissing, fileState }
}
