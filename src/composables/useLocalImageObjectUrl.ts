/**
 * @fileoverview Loads a user-selected local image through Rust IPC and exposes
 * a revokable object URL for use in CSS backgrounds and image elements.
 */
import { readonly, ref, toValue, watch, onBeforeUnmount, type MaybeRefOrGetter, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { logger } from '@shared/logger'
import { getErrorMessage } from '@shared/utils/errorMessage'

const IMAGE_MIME_BY_EXTENSION: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  webp: 'image/webp',
  bmp: 'image/bmp',
  gif: 'image/gif',
}

function isByte(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 255
}

/**
 * Tauri's IPC callback fallback serializes raw responses as JSON byte arrays
 * instead of preserving their ArrayBuffer representation.
 */
export function normalizeImageBytes(payload: unknown): Uint8Array {
  if (payload instanceof ArrayBuffer) return new Uint8Array(payload)
  if (payload instanceof Uint8Array) return payload
  if (Array.isArray(payload)) {
    const bytes = new Uint8Array(payload.length)
    for (let index = 0; index < payload.length; index += 1) {
      const value = payload[index]
      if (!isByte(value)) throw new TypeError('Invalid local image payload')
      bytes[index] = value
    }
    return bytes
  }
  throw new TypeError('Invalid local image payload')
}

export function inferImageMimeType(path: string): string {
  const extension = path.split(/[\\/]/).pop()?.split('.').pop()?.toLowerCase() ?? ''
  return IMAGE_MIME_BY_EXTENSION[extension] ?? 'application/octet-stream'
}

export function useLocalImageObjectUrl(pathSource: MaybeRefOrGetter<string>): Readonly<Ref<string>> {
  const objectUrl = ref('')
  let activeUrl = ''
  let requestId = 0

  function clearActiveUrl(): void {
    if (activeUrl) {
      URL.revokeObjectURL(activeUrl)
      activeUrl = ''
    }
    objectUrl.value = ''
  }

  watch(
    () => toValue(pathSource).trim(),
    async (path) => {
      const currentRequestId = ++requestId
      clearActiveUrl()

      if (!path) return

      try {
        const payload = await invoke<unknown>('read_local_image', { path })
        if (currentRequestId !== requestId) {
          return
        }

        const bytes = normalizeImageBytes(payload)
        const nextUrl = URL.createObjectURL(new Blob([bytes], { type: inferImageMimeType(path) }))
        activeUrl = nextUrl
        objectUrl.value = nextUrl
      } catch (e) {
        if (currentRequestId === requestId) {
          logger.warn('LocalImageObjectUrl.load', getErrorMessage(e))
        }
      }
    },
    { immediate: true },
  )

  onBeforeUnmount(() => {
    requestId += 1
    clearActiveUrl()
  })

  return readonly(objectUrl)
}
