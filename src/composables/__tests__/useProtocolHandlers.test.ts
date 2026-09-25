import { beforeEach, describe, expect, it, vi } from 'vitest'
const mockInvoke = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: mockInvoke }))
vi.mock('@shared/logger', () => ({ logger: { debug: vi.fn(), warn: vi.fn() } }))
import { useProtocolHandlers, type AssociationStatus } from '../useProtocolHandlers'
const snapshot = (state: AssociationStatus['state']): AssociationStatus => ({
  state,
  handler: null,
  error: null,
  canChange: true,
})

describe('native associations', () => {
  beforeEach(() => {
    mockInvoke.mockReset()
  })

  it('reports a protected default without claiming success', async () => {
    mockInvoke.mockRejectedValueOnce({ Protocol: 'manual_change_required' }).mockResolvedValueOnce(snapshot('other'))
    const associations = useProtocolHandlers()
    expect(await associations.setDefault('.torrent')).toEqual({ kind: 'manual' })
    expect(associations.status.value['.torrent']?.state).toBe('other')
  })

  it('serializes mutations until native verification completes', async () => {
    let complete!: () => void
    mockInvoke
      .mockReturnValueOnce(
        new Promise<void>((resolve) => {
          complete = resolve
        }),
      )
      .mockResolvedValueOnce(snapshot('unassigned'))
      .mockResolvedValue(snapshot('current'))
    const associations = useProtocolHandlers()
    const operation = associations.setDefault('magnet')
    expect(await associations.setDefault('ed2k')).toEqual({ kind: 'ignored' })
    await associations.refreshAll()
    expect(mockInvoke).toHaveBeenCalledTimes(1)
    complete()
    expect(await operation).toEqual({ kind: 'success' })
    expect(associations.busy.value).toBe(false)
    expect(associations.status.value.magnet?.state).toBe('current')
    expect(associations.status.value.ed2k?.state).toBe('current')
  })
})
