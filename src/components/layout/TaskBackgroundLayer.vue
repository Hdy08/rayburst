<script setup lang="ts">
/** @fileoverview Stable task-list background layer kept outside route transitions. */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useLocalImageObjectUrl } from '@/composables/useLocalImageObjectUrl'
import { useTaskBackgroundConfig } from '@/composables/useTaskBackgroundConfig'
import { logger } from '@shared/logger'
import { calculateCanvasPixelSize, calculateCoverSourceRect } from '@shared/utils/backgroundCanvas'

const props = withDefaults(defineProps<{ show: boolean; showDefaultIcon?: boolean }>(), { showDefaultIcon: true })

const taskBackground = useTaskBackgroundConfig()
const customBackgroundImageUrl = useLocalImageObjectUrl(() =>
  props.show ? taskBackground.backgroundImagePath.value : '',
)
const showCustomBackgroundImage = computed(() => props.show && customBackgroundImageUrl.value.length > 0)
const showDefaultBackgroundIcon = computed(
  () => props.show && props.showDefaultIcon && taskBackground.showDefaultBackgroundIcon.value,
)
const showTaskBackground = computed(() => showCustomBackgroundImage.value || showDefaultBackgroundIcon.value)
const backgroundCanvas = ref<HTMLCanvasElement | null>(null)
const backgroundLayerStyle = computed(() => ({
  '--task-background-content-opacity': String(taskBackground.backgroundOpacity.value),
  '--task-background-default-icon-opacity': String(taskBackground.defaultIconOpacity.value),
}))

let decodedBackgroundImage: HTMLImageElement | null = null
let drawFrameId: number | null = null
let imageRequestId = 0
let resizeObserver: ResizeObserver | null = null

function drawBackgroundImage(): void {
  drawFrameId = null
  const canvas = backgroundCanvas.value
  const image = decodedBackgroundImage
  if (!canvas || !image) return

  const bounds = canvas.getBoundingClientRect()
  const pixelSize = calculateCanvasPixelSize(bounds.width, bounds.height, window.devicePixelRatio)
  if (!pixelSize) return

  if (canvas.width !== pixelSize.width || canvas.height !== pixelSize.height) {
    canvas.width = pixelSize.width
    canvas.height = pixelSize.height
  }

  const context = canvas.getContext('2d')
  const sourceRect = calculateCoverSourceRect(
    image.naturalWidth,
    image.naturalHeight,
    pixelSize.width,
    pixelSize.height,
  )
  if (!context || !sourceRect) return

  context.clearRect(0, 0, pixelSize.width, pixelSize.height)
  context.imageSmoothingEnabled = true
  context.imageSmoothingQuality = 'high'
  context.drawImage(
    image,
    sourceRect.x,
    sourceRect.y,
    sourceRect.width,
    sourceRect.height,
    0,
    0,
    pixelSize.width,
    pixelSize.height,
  )
}

function scheduleBackgroundDraw(): void {
  if (drawFrameId !== null) return
  drawFrameId = window.requestAnimationFrame(drawBackgroundImage)
}

watch(
  customBackgroundImageUrl,
  (source) => {
    const requestId = ++imageRequestId
    decodedBackgroundImage = null
    if (!source) return

    const image = new Image()
    image.decoding = 'async'
    image.onload = () => {
      if (requestId !== imageRequestId) return
      decodedBackgroundImage = image
      scheduleBackgroundDraw()
    }
    image.onerror = () => {
      if (requestId === imageRequestId) {
        logger.warn('TaskBackgroundLayer.decode', 'Failed to decode custom background image')
      }
    }
    image.src = source
  },
  { immediate: true },
)

watch(backgroundCanvas, (canvas) => {
  resizeObserver?.disconnect()
  resizeObserver = null
  if (!canvas) return

  if (typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(scheduleBackgroundDraw)
    resizeObserver.observe(canvas)
  }
  scheduleBackgroundDraw()
})

onMounted(() => window.addEventListener('resize', scheduleBackgroundDraw))

onBeforeUnmount(() => {
  imageRequestId += 1
  decodedBackgroundImage = null
  resizeObserver?.disconnect()
  window.removeEventListener('resize', scheduleBackgroundDraw)
  if (drawFrameId !== null) window.cancelAnimationFrame(drawFrameId)
})
</script>

<template>
  <div
    class="task-background-layer"
    :class="{ 'is-visible': show }"
    :style="backgroundLayerStyle"
    aria-hidden="true"
    @dragstart.prevent
    @selectstart.prevent
  >
    <Transition name="task-background-content">
      <div v-if="showTaskBackground" class="task-background-content">
        <canvas v-if="showCustomBackgroundImage" ref="backgroundCanvas" class="task-background-image" />
        <div v-else class="task-background-icon" />
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.task-background-layer {
  grid-column: 2;
  grid-row: 3;
  position: relative;
  min-width: 0;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--main-bg);
  pointer-events: none;
  user-select: none;
  z-index: 0;
  opacity: 0;
  visibility: hidden;
  contain: layout paint;
}
.task-background-layer.is-visible {
  opacity: 1;
  visibility: visible;
}
.task-background-content {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}
.task-background-icon {
  max-width: 480px;
  width: 80%;
  aspect-ratio: 1;
  background: color-mix(in srgb, var(--m3-on-surface-variant) 85%, var(--m3-primary));
  mask: url('/logo.svg') center / contain no-repeat;
  opacity: var(--task-background-default-icon-opacity);
  user-select: none;
  -webkit-user-drag: none;
}
.task-background-image {
  display: block;
  width: 100%;
  height: 100%;
  image-rendering: auto;
  opacity: var(--task-background-content-opacity);
  user-select: none;
  -webkit-user-drag: none;
}
.task-background-content-enter-active,
.task-background-content-leave-active {
  transition: opacity 0.28s cubic-bezier(0.2, 0, 0, 1);
}
.task-background-content-enter-from,
.task-background-content-leave-to {
  opacity: 0;
}
</style>
