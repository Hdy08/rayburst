<script setup lang="ts">
/** Persistent footer controls bound to the active preference form. */
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { NButton, NIcon, NDropdown, type DropdownOption } from 'naive-ui'
import { RefreshOutline, EllipsisHorizontalOutline } from '@vicons/ionicons5'
import { useEngineRestart } from '@/composables/useEngineRestart'

const props = withDefaults(defineProps<{ isDirty: boolean; isValid?: boolean }>(), { isValid: true })
const emit = defineEmits<{ save: []; discard: [] }>()
const { t } = useI18n()
const { confirmManualRestart } = useEngineRestart()
const secondaryActions = computed<DropdownOption[]>(() => [
  { key: 'discard', label: t('preferences.discard'), disabled: !props.isDirty },
  { key: 'restart', label: t('preferences.engine-restart-btn') },
])
function selectAction(key: string | number) {
  if (key === 'discard' && props.isDirty) emit('discard')
  if (key === 'restart') void confirmManualRestart()
}
</script>

<template>
  <div class="form-actions">
    <NButton
      :type="isDirty && isValid ? 'primary' : 'default'"
      :disabled="!isDirty || !isValid"
      @click="emit('save')"
      >{{ t('preferences.save') }}</NButton
    >
    <NButton
      class="secondary-action"
      :type="isDirty ? 'error' : 'default'"
      :ghost="isDirty"
      :disabled="!isDirty"
      @click="emit('discard')"
      >{{ t('preferences.discard') }}</NButton
    >
    <NButton class="secondary-action" type="info" ghost @click="confirmManualRestart">
      <template #icon
        ><NIcon :size="16"><RefreshOutline /></NIcon
      ></template>
      {{ t('preferences.engine-restart-btn') }}
    </NButton>
    <NDropdown trigger="click" placement="top-start" :options="secondaryActions" @select="selectAction">
      <NButton class="compact-actions" quaternary :aria-label="t('task.more-actions')">
        <template #icon
          ><NIcon><EllipsisHorizontalOutline /></NIcon
        ></template>
      </NButton>
    </NDropdown>
  </div>
</template>

<style scoped>
.form-actions {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 46px;
  min-width: 0;
}
.form-actions > :first-child {
  min-width: 0;
  flex-shrink: 1;
}
.form-actions :deep(.n-button__content) {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
}
.compact-actions {
  display: none;
}
@container footer (max-width: 640px) {
  .secondary-action {
    display: none;
  }
  .compact-actions {
    display: inline-flex;
  }
  .form-actions {
    gap: 8px;
  }
}
</style>
