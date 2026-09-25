import { computed } from 'vue'
import { usePreferenceStore } from '@/stores/preference'
import { DEFAULT_APP_CONFIG } from '@shared/constants'
import { normalizeOpacityPercent } from '@shared/utils/opacity'

export function useTaskBackgroundConfig() {
  const preferenceStore = usePreferenceStore()
  const backgroundImagePath = computed(() => (preferenceStore.config.backgroundImagePath ?? '').trim())
  const hasCustomBackgroundImagePath = computed(() => backgroundImagePath.value.length > 0)
  const backgroundOpacity = computed(
    () => normalizeOpacityPercent(preferenceStore.config.backgroundOpacity, DEFAULT_APP_CONFIG.backgroundOpacity) / 100,
  )

  return {
    backgroundImagePath,
    backgroundOpacity,
    hasCustomBackgroundImagePath,
  }
}
