/** The layout owns the footer; the active form owns its commands. */
import { inject, provide, shallowRef, onMounted, onScopeDispose, type InjectionKey, type ShallowRef } from 'vue'

interface PreferenceActions {
  readonly routeName: string
  readonly isDirty: boolean
  readonly isValid: boolean
  save: () => void
  discard: () => void
}
const key: InjectionKey<ShallowRef<PreferenceActions | null>> = Symbol('preference-actions')

export function providePreferenceActions() {
  const actions = shallowRef<PreferenceActions | null>(null)
  provide(key, actions)
  return actions
}

export function registerPreferenceActions(actions: PreferenceActions) {
  const owner = inject(key)
  if (!owner) throw new Error('Preference actions require the application layout')
  onMounted(() => {
    owner.value = actions
  })
  onScopeDispose(() => {
    if (owner.value === actions) owner.value = null
  })
}
