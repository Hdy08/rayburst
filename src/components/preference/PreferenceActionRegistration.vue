<script setup lang="ts">
/** Register form commands without coupling footer visibility to page transitions. */
import { useRoute } from 'vue-router'
import { registerPreferenceActions } from '@/composables/usePreferenceActions'
const props = withDefaults(defineProps<{ isDirty: boolean; isValid?: boolean }>(), { isValid: true })
const emit = defineEmits<{ save: []; discard: [] }>()
registerPreferenceActions({
  routeName: String(useRoute().name),
  get isDirty() {
    return props.isDirty
  },
  get isValid() {
    return props.isValid
  },
  save: () => emit('save'),
  discard: () => emit('discard'),
})
</script>
<template><slot /></template>
