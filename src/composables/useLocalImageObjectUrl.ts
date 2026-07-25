/**
 * @fileoverview Prepares a user-selected image in Tauri's scoped asset cache
 * and exposes its URL for use in image elements.
 */
import { readonly, ref, toValue, watch, onBeforeUnmount, type MaybeRefOrGetter, type Ref } from 'vue'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { logger } from '@shared/logger'
import { getErrorMessage } from '@shared/utils/errorMessage'

export function useLocalImageObjectUrl(pathSource: MaybeRefOrGetter<string>): Readonly<Ref<string>> {
  const imageUrl = ref('')
  let requestId = 0

  function clearImageUrl(): void {
    imageUrl.value = ''
  }

  watch(
    () => toValue(pathSource).trim(),
    async (path) => {
      const currentRequestId = ++requestId
      clearImageUrl()

      if (!path) return

      try {
        const cachedPath = await invoke<string>('prepare_local_background', { path })
        if (currentRequestId !== requestId) {
          return
        }

        if (typeof cachedPath !== 'string' || !cachedPath) {
          throw new TypeError('Invalid cached local image path')
        }
        imageUrl.value = convertFileSrc(cachedPath)
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
    clearImageUrl()
  })

  return readonly(imageUrl)
}
