import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { NPagination } from 'naive-ui'
import { buildAppColorTokens, buildColorSchemeTheme, resolveColorScheme } from '@shared/utils/colorScheme'
import { buildTaskPaginationTheme } from '../taskPaginationTheme'

describe('task pagination opacity', () => {
  it('applies and updates opacity on arrow borders, disabled backgrounds, and the page-size picker', async () => {
    const tokens = buildAppColorTokens(buildColorSchemeTheme(resolveColorScheme(undefined, undefined)), false)
    const wrapper = mount(NPagination, {
      props: {
        page: 1,
        pageCount: 4,
        pageSize: 20,
        pageSizes: [20, 40],
        showSizePicker: true,
        themeOverrides: buildTaskPaginationTheme(tokens, '25%'),
      },
    })

    const paginationStyle = wrapper.find('.n-pagination').attributes('style')
    const pickerStyle = wrapper.find('.n-base-selection').attributes('style')
    expect(paginationStyle).toContain(
      `--n-button-border: 1px solid color-mix(in srgb, ${tokens.outlineVariant} 25%, transparent)`,
    )
    expect(paginationStyle).toContain(
      `--n-item-color-disabled: color-mix(in srgb, ${tokens.surfaceContainerLow} 25%, transparent)`,
    )
    expect(pickerStyle).toContain(`--n-color: color-mix(in srgb, ${tokens.surfaceContainer} 25%, transparent)`)
    expect(pickerStyle).toContain(`--n-border: 1px solid color-mix(in srgb, ${tokens.outlineVariant} 25%, transparent)`)

    await wrapper.setProps({ themeOverrides: buildTaskPaginationTheme(tokens, '0%') })
    expect(wrapper.find('.n-pagination').attributes('style')).toContain(
      `--n-button-border: 1px solid color-mix(in srgb, ${tokens.outlineVariant} 0%, transparent)`,
    )
    expect(wrapper.find('.n-base-selection').attributes('style')).toContain(
      `--n-color: color-mix(in srgb, ${tokens.surfaceContainer} 0%, transparent)`,
    )
    wrapper.unmount()
  })
})
