import { computed } from 'vue'
import { usePreferenceStore } from '@/stores/preference'
import { DEFAULT_APP_CONFIG } from '@shared/constants'
import { normalizeOpacityPercent } from '@shared/utils/opacity'

const TASK_LIST_WATERMARK_OPACITY = 0.35

export function useTaskBackgroundConfig() {
  const preferenceStore = usePreferenceStore()
  const backgroundImagePath = computed(() => (preferenceStore.config.backgroundImagePath ?? '').trim())
  const hasCustomBackgroundImagePath = computed(() => backgroundImagePath.value.length > 0)
  const showDefaultBackgroundIcon = computed(
    () => preferenceStore.config.taskListWatermark && !hasCustomBackgroundImagePath.value,
  )
  const isTaskBackgroundConfigured = computed(
    () => hasCustomBackgroundImagePath.value || showDefaultBackgroundIcon.value,
  )
  const backgroundOpacity = computed(
    () => normalizeOpacityPercent(preferenceStore.config.backgroundOpacity, DEFAULT_APP_CONFIG.backgroundOpacity) / 100,
  )
  const defaultIconOpacity = computed(() => TASK_LIST_WATERMARK_OPACITY)

  return {
    backgroundImagePath,
    backgroundOpacity,
    defaultIconOpacity,
    hasCustomBackgroundImagePath,
    isTaskBackgroundConfigured,
    showDefaultBackgroundIcon,
  }
}
