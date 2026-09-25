import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import PreferenceAssociations from '../PreferenceAssociations.vue'

const invoke = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ onFocusChanged: vi.fn().mockResolvedValue(vi.fn()) }),
}))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/composables/usePlatform', () => ({ usePlatform: () => ({ isWindows: true }) }))
vi.mock('@/composables/useAppMessage', () => ({
  useAppMessage: () => ({ success: vi.fn(), warning: vi.fn(), info: vi.fn(), error: vi.fn() }),
}))
vi.mock('@shared/utils', () => ({ writeAppClipboardText: vi.fn() }))
vi.mock('@shared/logger', () => ({ logger: { debug: vi.fn(), warn: vi.fn() } }))
vi.mock('naive-ui', () => ({
  NButton: { props: ['disabled', 'loading'], template: '<button :disabled="disabled"><slot /></button>' },
  NDivider: { template: '<div><slot /></div>' },
  NFormItem: { props: ['label'], template: '<div><label>{{ label }}</label><slot /><slot name="feedback" /></div>' },
  NTooltip: { template: '<div><slot name="trigger" /><slot /></div>' },
  NSkeleton: { template: '<span />' },
  NInput: { template: '<div><slot name="prefix" /></div>' },
  NInputGroup: { template: '<div><slot /></div>' },
  NIcon: { template: '<span><slot /></span>' },
}))
beforeEach(() => {
  invoke.mockReset()
})

describe('association settings', () => {
  it('keeps file and Rayburst repair actions available after a failed query', async () => {
    invoke.mockResolvedValue({ state: 'error', handler: null, error: 'native failure', canChange: true })
    const wrapper = mount(PreferenceAssociations)
    await flushPromises()
    expect(wrapper.text()).toContain('BitTorrent [.torrent]')
    expect(wrapper.text()).toContain('Rayburst [rayburst://]')
    const repair = wrapper.get('button[aria-label="Rayburst [rayburst://]: preferences.association-repair"]')
    expect(repair.attributes('disabled')).toBeUndefined()
    await repair.trigger('click')
    await flushPromises()
    expect(invoke).toHaveBeenCalledWith('set_default_protocol_client', { protocol: 'rayburst' })
    expect(invoke).not.toHaveBeenCalledWith('open_default_apps_settings')
    const settings = wrapper.findAll('button').find((button) => button.text() === 'preferences.association-settings')
    expect(settings).toBeDefined()
    await settings!.trigger('click')
    expect(invoke).toHaveBeenCalledWith('open_default_apps_settings')
    wrapper.unmount()
  })
})
