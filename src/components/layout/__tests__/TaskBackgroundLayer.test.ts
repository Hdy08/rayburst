import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick, reactive } from 'vue'

const state = vi.hoisted(() => ({
  preferenceStore: {
    config: {
      backgroundImagePath: '',
      backgroundOpacity: 35,
    },
  },
  imageUrl: { value: '', __v_isRef: true },
}))

vi.mock('@/stores/preference', () => ({
  usePreferenceStore: () => state.preferenceStore,
}))

vi.mock('@/composables/useLocalImageObjectUrl', () => ({
  useLocalImageObjectUrl: () => state.imageUrl,
}))

import TaskBackgroundLayer from '../TaskBackgroundLayer.vue'

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('TaskBackgroundLayer', () => {
  beforeEach(() => {
    state.preferenceStore.config = reactive({
      backgroundImagePath: '',
      backgroundOpacity: 35,
    })
    state.imageUrl.value = ''
  })

  it('keeps the background layer hidden when no custom image is selected', () => {
    const wrapper = mount(TaskBackgroundLayer, { props: { show: true } })

    expect(wrapper.find('.task-background-layer').classes()).not.toContain('is-visible')
    expect(wrapper.find('.task-background-content').exists()).toBe(false)
    wrapper.unmount()
  })

  it('keeps the layer mounted but hidden off task pages', () => {
    const wrapper = mount(TaskBackgroundLayer, { props: { show: false } })

    expect(wrapper.find('.task-background-layer').exists()).toBe(true)
    expect(wrapper.find('.task-background-layer').classes()).not.toContain('is-visible')
  })

  it('renders a custom background image across the full layer', () => {
    state.preferenceStore.config.backgroundImagePath = 'C:\\Users\\me\\Pictures\\background.png'
    state.imageUrl.value = 'blob:background'

    const wrapper = mount(TaskBackgroundLayer, { props: { show: true } })
    const backgroundImage = wrapper.find('.task-background-image')

    expect(backgroundImage.element.tagName).toBe('CANVAS')
    expect(wrapper.find('.task-background-layer').classes()).toContain('is-visible')
    wrapper.unmount()
  })

  it('draws custom backgrounds with high-quality image smoothing', async () => {
    state.preferenceStore.config.backgroundImagePath = 'C:\\Users\\me\\Pictures\\background.png'
    state.imageUrl.value = 'blob:background'

    const imageFixture: { value: HTMLImageElement | null } = { value: null }
    let scheduledFrame: FrameRequestCallback | null = null
    vi.stubGlobal('Image', function MockImage() {
      const image = document.createElement('img')
      Object.defineProperties(image, {
        naturalWidth: { value: 4000 },
        naturalHeight: { value: 2000 },
      })
      imageFixture.value = image
      return image
    })
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      scheduledFrame = callback
      return 1
    })

    const wrapper = mount(TaskBackgroundLayer, { props: { show: true } })
    await nextTick()
    const canvas = wrapper.find('.task-background-image').element
    const decodedImage = imageFixture.value
    if (!(canvas instanceof HTMLCanvasElement) || !decodedImage) {
      throw new Error('Expected a canvas and decoded image fixture')
    }

    const context = {
      clearRect: vi.fn(),
      drawImage: vi.fn(),
      imageSmoothingEnabled: false,
      imageSmoothingQuality: 'low',
    }
    Object.defineProperty(canvas, 'getBoundingClientRect', {
      value: () => ({ width: 1000, height: 500 }),
    })
    Object.defineProperty(canvas, 'getContext', { value: () => context })

    const runScheduledFrame = (): void => {
      const frame = scheduledFrame
      scheduledFrame = null
      frame?.(0)
    }
    runScheduledFrame()
    decodedImage.onload?.call(decodedImage, new Event('load'))
    runScheduledFrame()

    expect(context.imageSmoothingEnabled).toBe(true)
    expect(context.imageSmoothingQuality).toBe('high')
    expect(context.drawImage).toHaveBeenCalledOnce()
    wrapper.unmount()
  })

  it('applies configured background opacity to the image content', () => {
    state.preferenceStore.config.backgroundOpacity = 75

    const wrapper = mount(TaskBackgroundLayer, { props: { show: true } })

    const layerStyle = wrapper.find('.task-background-layer').attributes('style')
    expect(layerStyle).toContain('--task-background-content-opacity: 0.75')
    wrapper.unmount()
  })

  it('keeps the full-window background base visible while the custom image loads', () => {
    state.preferenceStore.config.backgroundImagePath = 'C:\\Users\\me\\Pictures\\background.png'
    const wrapper = mount(TaskBackgroundLayer, { props: { show: true } })

    expect(wrapper.find('.task-background-layer').classes()).toContain('is-visible')
    expect(wrapper.find('.task-background-content').exists()).toBe(false)
    expect(wrapper.find('.task-background-layer').attributes('style')).not.toContain('--task-background-left-inset')
    wrapper.unmount()
  })
})
