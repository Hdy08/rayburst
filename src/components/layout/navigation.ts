/** Shared navigation labels and destinations. */
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { ListOutline, PlayOutline, AlertCircleOutline, CheckmarkDoneOutline } from '@vicons/ionicons5'

export function useTaskDestinations() {
  const { t } = useI18n()
  return computed(
    () =>
      [
        { key: 'all', label: t('task.scope-all'), icon: ListOutline },
        { key: 'progress', label: t('task.scope-progress'), icon: PlayOutline },
        { key: 'failed', label: t('task.scope-failed'), icon: AlertCircleOutline },
        { key: 'completed', label: t('task.scope-completed'), icon: CheckmarkDoneOutline },
      ] as const,
  )
}

export const preferenceDestinations = ['general', 'downloads', 'bt', 'ed2k', 'network', 'advanced'] as const
