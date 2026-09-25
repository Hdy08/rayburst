/** @fileoverview Regression coverage for native component surface derivation. */
import { describe, expect, it } from 'vitest'
import { lightTheme, darkTheme } from 'naive-ui'
import { argbFromHex, redFromArgb, greenFromArgb, blueFromArgb } from '@material/material-color-utilities'
import { buildNaiveTheme } from '../useColorScheme'
import { buildAppColorTokens, buildColorSchemeTheme, resolveColorScheme } from '@shared/utils/colorScheme'

describe('component surface colors', () => {
  it('derives table headers and description labels from the selected palette in every context', () => {
    const palette = buildColorSchemeTheme(resolveColorScheme('rayburst', undefined))
    for (const dark of [false, true]) {
      const tokens = buildAppColorTokens(palette, dark)
      const base = dark ? darkTheme : lightTheme
      const common = { ...base.common!, ...buildNaiveTheme(tokens).common }
      for (const component of [base.DataTable!, base.Descriptions!]) {
        const colors = component.self!(common)
        for (const key of ['thColor', 'thColorModal', 'thColorPopover'] as const) {
          const argb = argbFromHex(tokens.surfaceContainerLow)
          expect(colors[key]).toBe(`rgba(${redFromArgb(argb)}, ${greenFromArgb(argb)}, ${blueFromArgb(argb)}, 1)`)
        }
      }
    }
  })
})
