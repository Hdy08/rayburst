import { defineComponent, h, nextTick, ref, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()
const convertFileSrcMock = vi.fn((path: string) => `http://asset.localhost/${encodeURIComponent(path)}`)
const warnMock = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  convertFileSrc: (...args: [string]) => convertFileSrcMock(...args),
}))

vi.mock('@shared/logger', () => ({
  logger: { warn: (...args: unknown[]) => warnMock(...args) },
}))

import { useLocalImageObjectUrl } from '../useLocalImageObjectUrl'

function mountHarness(initialPath: string): {
  imagePath: Ref<string>
  imageUrl: Readonly<Ref<string>>
  unmount: () => void
} {
  const imagePath = ref(initialPath)
  let imageUrl!: Readonly<Ref<string>>
  const wrapper = mount(
    defineComponent({
      setup() {
        imageUrl = useLocalImageObjectUrl(imagePath)
        return () => h('div', imageUrl.value)
      },
    }),
  )

  return {
    imagePath,
    imageUrl,
    unmount: () => wrapper.unmount(),
  }
}

describe('useLocalImageObjectUrl', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('prepares a local image and exposes a scoped asset URL', async () => {
    const cachedPath = 'C:\\Users\\me\\AppData\\Local\\MotrixNext\\cache\\motrix-background\\background.png'
    invokeMock.mockResolvedValue(cachedPath)

    const { imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\background.png')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('prepare_local_background', {
      path: 'C:\\Users\\me\\Pictures\\background.png',
    })
    expect(convertFileSrcMock).toHaveBeenCalledWith(cachedPath)
    expect(imageUrl.value).toBe(`http://asset.localhost/${encodeURIComponent(cachedPath)}`)
  })

  it('clears the scoped asset URL when the path is cleared', async () => {
    invokeMock.mockResolvedValue('C:\\Users\\me\\AppData\\Local\\MotrixNext\\cache\\motrix-background\\background.webp')

    const { imagePath, imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\background.webp')
    await flushPromises()

    imagePath.value = ''
    await nextTick()

    expect(imageUrl.value).toBe('')
  })

  it('does not expose stale asset URLs when paths change during loading', async () => {
    let resolveFirst!: (value: string) => void
    invokeMock
      .mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            resolveFirst = resolve
          }),
      )
      .mockResolvedValueOnce('C:\\Users\\me\\AppData\\Local\\MotrixNext\\cache\\motrix-background\\background-new.jpg')

    const { imagePath, imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\old.png')
    imagePath.value = 'C:\\Users\\me\\Pictures\\new.jpg'
    await nextTick()
    await flushPromises()

    expect(imageUrl.value).toBe(
      `http://asset.localhost/${encodeURIComponent(
        'C:\\Users\\me\\AppData\\Local\\MotrixNext\\cache\\motrix-background\\background-new.jpg',
      )}`,
    )

    resolveFirst('C:\\Users\\me\\AppData\\Local\\MotrixNext\\cache\\motrix-background\\background-old.png')
    await flushPromises()

    expect(convertFileSrcMock).toHaveBeenCalledTimes(1)
  })

  it('clears the scoped asset URL on unmount', async () => {
    invokeMock.mockResolvedValue('C:\\Users\\me\\AppData\\Local\\MotrixNext\\cache\\motrix-background\\background.gif')

    const { imageUrl, unmount } = mountHarness('C:\\Users\\me\\Pictures\\background.gif')
    await flushPromises()

    unmount()

    expect(imageUrl.value).toBe('')
  })

  it('logs image load failures without retaining a stale URL', async () => {
    invokeMock.mockRejectedValue(new Error('image too large'))

    const { imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\background.png')
    await flushPromises()

    expect(imageUrl.value).toBe('')
    expect(warnMock).toHaveBeenCalledWith('LocalImageObjectUrl.load', 'image too large')
  })

  it('rejects an invalid cached image path', async () => {
    invokeMock.mockResolvedValue(null)

    const { imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\background.png')
    await flushPromises()

    expect(imageUrl.value).toBe('')
    expect(warnMock).toHaveBeenCalledWith('LocalImageObjectUrl.load', 'Invalid cached local image path')
  })
})
