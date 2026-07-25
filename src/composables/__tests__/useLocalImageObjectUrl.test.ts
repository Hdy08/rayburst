import { defineComponent, h, nextTick, ref, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()
const warnMock = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

vi.mock('@shared/logger', () => ({
  logger: { warn: (...args: unknown[]) => warnMock(...args) },
}))

import { inferImageMimeType, normalizeImageBytes, useLocalImageObjectUrl } from '../useLocalImageObjectUrl'

let objectUrlIndex = 0
const createObjectURLMock = vi.fn((_blob: Blob) => `blob:background-${++objectUrlIndex}`)
const revokeObjectURLMock = vi.fn((_url: string) => undefined)

function installUrlMocks(): void {
  Object.defineProperty(URL, 'createObjectURL', {
    configurable: true,
    value: createObjectURLMock,
  })
  Object.defineProperty(URL, 'revokeObjectURL', {
    configurable: true,
    value: revokeObjectURLMock,
  })
}

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
    objectUrlIndex = 0
    installUrlMocks()
  })

  it('infers image MIME types from local paths', () => {
    expect(inferImageMimeType('C:\\Users\\me\\Pictures\\cover.PNG')).toBe('image/png')
    expect(inferImageMimeType('/home/me/wallpaper.jpeg')).toBe('image/jpeg')
    expect(inferImageMimeType('/home/me/wallpaper.unknown')).toBe('application/octet-stream')
  })

  it('normalizes JSON byte arrays used by the Tauri IPC callback fallback', () => {
    const pngHeader = [137, 80, 78, 71, 13, 10, 26, 10]

    expect(Array.from(normalizeImageBytes(pngHeader))).toEqual(pngHeader)
  })

  it('rejects malformed local image payloads', () => {
    expect(() => normalizeImageBytes([137, 80, -1])).toThrow('Invalid local image payload')
    expect(() => normalizeImageBytes([137, , 80])).toThrow('Invalid local image payload')
  })

  it('loads a local image through Rust IPC and exposes an object URL', async () => {
    invokeMock.mockResolvedValue(Uint8Array.from([1, 2, 3]).buffer)

    const { imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\background.png')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('read_local_image', {
      path: 'C:\\Users\\me\\Pictures\\background.png',
    })
    expect(createObjectURLMock).toHaveBeenCalledTimes(1)
    const blobArg = createObjectURLMock.mock.calls[0][0]
    expect(blobArg).toBeInstanceOf(Blob)
    if (!(blobArg instanceof Blob)) throw new Error('expected Blob argument')
    expect(blobArg.type).toBe('image/png')
    expect(imageUrl.value).toBe('blob:background-1')
  })

  it('preserves binary image data from a JSON byte-array IPC fallback', async () => {
    const pngHeader = [137, 80, 78, 71, 13, 10, 26, 10]
    invokeMock.mockResolvedValue(pngHeader)

    const { imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\background.png')
    await flushPromises()

    const blobArg = createObjectURLMock.mock.calls[0][0]
    expect(blobArg).toBeInstanceOf(Blob)
    if (!(blobArg instanceof Blob)) throw new Error('expected Blob argument')
    expect(Array.from(new Uint8Array(await blobArg.arrayBuffer()))).toEqual(pngHeader)
    expect(imageUrl.value).toBe('blob:background-1')
  })

  it('revokes the active object URL when the path is cleared', async () => {
    invokeMock.mockResolvedValue(Uint8Array.from([1, 2, 3]).buffer)

    const { imagePath, imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\background.webp')
    await flushPromises()

    imagePath.value = ''
    await nextTick()

    expect(revokeObjectURLMock).toHaveBeenCalledWith('blob:background-1')
    expect(imageUrl.value).toBe('')
  })

  it('does not expose stale object URLs when paths change during loading', async () => {
    let resolveFirst!: (value: ArrayBuffer) => void
    invokeMock
      .mockImplementationOnce(
        () =>
          new Promise<ArrayBuffer>((resolve) => {
            resolveFirst = resolve
          }),
      )
      .mockResolvedValueOnce(Uint8Array.from([4, 5, 6]).buffer)

    const { imagePath, imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\old.png')
    imagePath.value = 'C:\\Users\\me\\Pictures\\new.jpg'
    await nextTick()
    await flushPromises()

    expect(imageUrl.value).toBe('blob:background-1')

    resolveFirst(Uint8Array.from([1, 2, 3]).buffer)
    await flushPromises()

    expect(imageUrl.value).toBe('blob:background-1')
    expect(createObjectURLMock).toHaveBeenCalledTimes(1)
    expect(revokeObjectURLMock).not.toHaveBeenCalled()
  })

  it('revokes the active object URL on unmount', async () => {
    invokeMock.mockResolvedValue(Uint8Array.from([1, 2, 3]).buffer)

    const { unmount } = mountHarness('C:\\Users\\me\\Pictures\\background.gif')
    await flushPromises()

    unmount()

    expect(revokeObjectURLMock).toHaveBeenCalledWith('blob:background-1')
  })

  it('logs image load failures without retaining a stale URL', async () => {
    invokeMock.mockRejectedValue(new Error('image too large'))

    const { imageUrl } = mountHarness('C:\\Users\\me\\Pictures\\background.png')
    await flushPromises()

    expect(imageUrl.value).toBe('')
    expect(warnMock).toHaveBeenCalledWith('LocalImageObjectUrl.load', 'image too large')
  })
})
