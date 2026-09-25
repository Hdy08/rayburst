<script setup lang="ts">
/** @fileoverview Task list view, task actions, and file delete confirmation. */
import { computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useTaskStore } from '@/stores/task'
import { useTaskSelectionStore } from '@/stores/taskSelection'
import { usePreferenceStore } from '@/stores/preference'

import { useTaskActions } from '@/composables/useTaskActions'

import { useDialog } from 'naive-ui'
import { useAppMessage } from '@/composables/useAppMessage'
import TaskList from '@/components/task/TaskList.vue'
import TaskDetail from '@/components/task/TaskDetail.vue'
import TaskEmptyBrand from '@/components/task/TaskEmptyBrand.vue'

const props = withDefaults(defineProps<{ status?: string }>(), { status: 'all' })

const { t } = useI18n()
const taskStore = useTaskStore()
const preferenceStore = usePreferenceStore()
const showEmptyBrand = computed(() => preferenceStore.config.showLogoWhenEmpty && taskStore.isCurrentListEmpty)
const dialog = useDialog()
const message = useAppMessage()

const {
  handlePauseTask,
  handleResumeTask,
  handleRetryTask,
  handleRedownloadTask,
  handleFinishSharing,
  handleFinishMedia,
  handleDeleteTask,
  handleDeleteRecord,
  handleCopyLink,
  handleShowInfo,
  handleShowInFolder,
  handleOpenFile,
  handleSelectFiles,
} = useTaskActions({
  taskStore,
  preferenceConfig: () => preferenceStore.config,
  t,
  dialog,
  message,
  requestMagnetSelection: (gid) => useTaskSelectionStore().request({ kind: 'bt', gid }),
})

watch(
  () => props.status,
  (status) => {
    void taskStore.changeCurrentList(status)
  },
  { immediate: true },
)
</script>

<template>
  <div class="task-view">
    <div class="panel-body">
      <TaskEmptyBrand :show="showEmptyBrand" />
      <div class="panel-content">
        <TaskList
          @pause="handlePauseTask"
          @resume="handleResumeTask"
          @retry="handleRetryTask"
          @redownload="handleRedownloadTask"
          @finish-sharing="handleFinishSharing"
          @finish-media="handleFinishMedia"
          @delete="handleDeleteTask"
          @delete-record="handleDeleteRecord"
          @copy-link="handleCopyLink"
          @show-info="handleShowInfo"
          @folder="handleShowInFolder"
          @open-file="handleOpenFile"
          @select-files="handleSelectFiles"
        />
      </div>
    </div>
    <TaskDetail
      :show="taskStore.taskDetailVisible"
      :task="taskStore.currentTaskItem"
      :files="taskStore.currentTaskFiles"
      @close="taskStore.hideTaskDetail()"
    />
  </div>
</template>

<style scoped>
.task-view {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.panel-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.panel-content {
  padding: 0;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
}
</style>
