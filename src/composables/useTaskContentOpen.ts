import type { Aria2Task } from '@shared/types'
import { checkTaskIsSharing } from '@shared/utils/task'

/** Open completed content without intercepting controls or text selection. */
export function canOpenTaskContent(task: Aria2Task, event: MouseEvent): boolean {
  if (task.status !== 'complete' && !checkTaskIsSharing(task)) return false
  if (
    event.target instanceof Element &&
    event.target.closest('button, a, input, select, textarea, [role="button"], [contenteditable="true"]')
  )
    return false
  return !window.getSelection()?.toString()
}
